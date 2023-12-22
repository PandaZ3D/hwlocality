//! Modifying a loaded Topology
//!
//! In an ideal world, modifying a topology would just be a matter of calling
//! methods on an `&mut Topology`. Alas, this binding has to make it a little
//! more complicated than that due to the following reasons:
//!
//! - hwloc employs lazy caching patterns in such a way that after editing the
//!   topology, calling functions on an `*const hwloc_topology` may modify it
//!   in a thread-unsafe way. This is deeply at odds with the general design of
//!   the Rust aliasing model, and accounting for it by simply marking topology
//!   objects as internally mutable would result in major usability regressions
//!   (e.g. [`TopologyObject`] could not be [`Sync`]).
//! - Many hwloc topology editing functions take one or more `*const hwloc_obj`
//!   as a parameter. This is at odds with the simplest way to model topology
//!   object lookup in Rust, namely as borrows from the source [`Topology`],
//!   because once you have borrowed an `&TopologyObject` from a `&Topology`,
//!   you cannot call methods that require `&mut Topology` anymore. Working
//!   around this issue requires pointer-based unsafe code, carefully written
//!   so as not to violate Rust's aliasing model.
//! - While all of this would be workable through a sufficiently complicated API
//!   that lets the binding use internal mutability everywhere and delay
//!   creation of Rust references until the very moment where they are needed,
//!   one must bear in mind that topology editing is ultimately a niche feature
//!   which most hwloc users will never reach for. Common sense demands that it
//!   is the niche editing feature that takes an ergonomic and complexity hit,
//!   not the everyday topology queries.
//!
//! Therefore, topology editing is carried out using a dedicated
//! [`TopologyEditor`] type, defined in this module, which unfortunately has
//! sub-optimal ergonomics as a result of making the regular [`Topology`] type
//! as easy to use, cleanly implemented and feature-complete as it should be.

use crate::{
    bitmap::{Bitmap, BitmapKind, OwnedSpecializedBitmap, SpecializedBitmap},
    cpu::cpuset::CpuSet,
    errors::{self, ForeignObjectError, HybridError, NulError, ParameterError, RawHwlocError},
    ffi::{
        string::LibcString,
        transparent::{AsInner, AsNewtype},
    },
    memory::nodeset::NodeSet,
    object::{attributes::GroupAttributes, TopologyObject},
    topology::Topology,
};
#[cfg(doc)]
use crate::{
    object::types::ObjectType,
    topology::builder::{BuildFlags, TopologyBuilder, TypeFilter},
};
use bitflags::bitflags;
use derive_more::Display;
use enum_iterator::Sequence;
use hwlocality_sys::{
    hwloc_restrict_flags_e, hwloc_topology, HWLOC_ALLOW_FLAG_ALL, HWLOC_ALLOW_FLAG_CUSTOM,
    HWLOC_ALLOW_FLAG_LOCAL_RESTRICTIONS, HWLOC_RESTRICT_FLAG_ADAPT_IO,
    HWLOC_RESTRICT_FLAG_ADAPT_MISC, HWLOC_RESTRICT_FLAG_BYNODESET,
    HWLOC_RESTRICT_FLAG_REMOVE_CPULESS, HWLOC_RESTRICT_FLAG_REMOVE_MEMLESS,
};
use libc::{EINVAL, ENOMEM};
#[allow(unused)]
#[cfg(test)]
use similar_asserts::assert_eq;
use std::{
    fmt::{self, Write},
    panic::{AssertUnwindSafe, UnwindSafe},
    ptr::{self, NonNull},
};
use thiserror::Error;

/// # Modifying a loaded `Topology`
//
// --- Implementation details ---
//
// Upstream docs: https://hwloc.readthedocs.io/en/v2.9/group__hwlocality__tinker.html
impl Topology {
    /// Modify this topology
    ///
    /// hwloc employs lazy caching patterns that do not interact well with
    /// Rust's shared XOR mutable aliasing model. This API lets you safely
    /// modify the active `Topology` through a [`TopologyEditor`] proxy object,
    /// with the guarantee that by the time `Topology::edit()` returns, the
    /// `Topology` will be back in a state where it is safe to use `&self` again.
    ///
    /// In general, the hwlocality binding optimizes the ergonomics and
    /// performance of reading and using topologies at the expense of making
    /// them harder and slower to edit. If a strong need for easier or more
    /// efficient topology editing emerged, the right thing to do would
    /// probably be to set up an alternate hwloc Rust binding optimized for
    /// that, sharing as much code as possible with hwlocality.
    #[doc(alias = "hwloc_topology_refresh")]
    pub fn edit<R>(&mut self, edit: impl UnwindSafe + FnOnce(&mut TopologyEditor<'_>) -> R) -> R {
        // Set up topology editing
        let mut editor = TopologyEditor::new(self);
        let mut editor = AssertUnwindSafe(&mut editor);

        // Run the user-provided edit callback, catching panics
        let result = std::panic::catch_unwind(move || edit(&mut editor));

        // Force eager evaluation of all caches
        self.refresh();

        // Return user callback result or resume unwinding as appropriate
        match result {
            Ok(result) => result,
            Err(e) => std::panic::resume_unwind(e),
        }
    }

    /// Force eager evaluation of all lazily evaluated caches in preparation for
    /// using or exposing &self
    ///
    /// # Aborts
    ///
    /// A process abort will occur if this fails as we must not let an invalid
    /// `Topology` state escape, not even via unwinding, as that would result in
    /// undefined behavior (mutation which the compiler assumes will not happen).
    #[allow(clippy::print_stderr)]
    pub(crate) fn refresh(&mut self) {
        // Evaluate all the caches
        // SAFETY: - Topology is trusted to contain a valid ptr (type invariant)
        //         - hwloc ops are trusted to keep *mut parameters in a
        //           valid state unless stated otherwise
        let result = errors::call_hwloc_int_normal("hwloc_topology_refresh", || unsafe {
            hwlocality_sys::hwloc_topology_refresh(self.as_mut_ptr())
        });
        if let Err(e) = result {
            eprintln!("ERROR: Failed to refresh topology ({e}), so it's stuck in a state that violates Rust aliasing rules. Must abort...");
            std::process::abort()
        }

        // Check topology for correctness before exposing it
        if cfg!(debug_assertions) {
            // SAFETY: - Topology is trusted to contain a valid ptr (type invariant)
            //         - hwloc ops are trusted not to modify *const parameters
            unsafe { hwlocality_sys::hwloc_topology_check(self.as_ptr()) }
        }
    }
}

/// Proxy for modifying a `Topology`
///
/// This proxy object is carefully crafted to only allow operations that are
/// safe while modifying a topology and minimize the number of times the hwloc
/// lazy caches will need to be refreshed.
///
/// The API is broken down into sections roughly following the structure of the
/// upstream hwloc documentation:
///
/// - [General-purpose utilities](#general-purpose-utilities)
/// - [Basic modifications](#basic-modifications)
#[cfg_attr(
    feature = "hwloc-2_5_0",
    doc = "- [Add distances between objects](#add-distances-between-objects) (hwloc 2.5+)"
)]
/// - [Remove distances between objects](#remove-distances-between-objects)
/// - [Managing memory attributes](#managing-memory-attributes)
#[cfg_attr(
    feature = "hwloc-2_4_0",
    doc = "- [Kinds of CPU cores](#kinds-of-cpu-cores) (hwloc 2.4+)"
)]
//
// --- Implementation details
//
// Not all of the TopologyEditor API is implemented in the core editor.rs
// module. Instead, functionality which is very strongly related to one other
// code module is implemented in that module, leaving the editor module focused
// on basic lifecycle and cross-cutting issues.
#[derive(Debug)]
pub struct TopologyEditor<'topology>(&'topology mut Topology);

/// # General-purpose utilities
impl<'topology> TopologyEditor<'topology> {
    /// Wrap an `&mut Topology` into a topology editor
    pub(crate) fn new(topology: &'topology mut Topology) -> Self {
        Self(topology)
    }

    /// Get a shared reference to the inner Topology
    ///
    /// This requires rebuilding inner caches, which can be costly. Prefer
    /// accessing the topology before or after editing it if possible.
    pub fn topology(&mut self) -> &Topology {
        self.topology_mut().refresh();
        self.topology_mut()
    }

    /// Get a mutable reference to the inner Topology
    pub(crate) fn topology_mut(&mut self) -> &mut Topology {
        self.0
    }

    /// Contained hwloc topology pointer (for interaction with hwloc)
    pub(crate) fn topology_mut_ptr(&mut self) -> *mut hwloc_topology {
        self.topology_mut().as_mut_ptr()
    }
}

/// # Basic modifications
//
// --- Implementation details ---
//
// Upstream docs: https://hwloc.readthedocs.io/en/v2.9/group__hwlocality__tinker.html
impl<'topology> TopologyEditor<'topology> {
    /// Restrict the topology to the given CPU set or nodeset
    ///
    /// The topology is modified so as to remove all objects that are not
    /// included (or partially included) in the specified [`CpuSet`] or
    /// [`NodeSet`] set. All objects CPU and node sets are restricted
    /// accordingly.
    ///
    /// Restricting the topology removes some locality information, hence the
    /// remaining objects may get reordered (including PUs and NUMA nodes), and
    /// their logical indices may change.
    ///
    /// This call may not be reverted by restricting back to a larger set. Once
    /// dropped during restriction, objects may not be brought back, except by
    /// loading another topology with [`Topology::new()`] or [`TopologyBuilder`].
    ///
    /// # Errors
    ///
    /// It is an error to attempt to remove all CPUs or NUMA nodes from a
    /// topology using a `set` that has no intersection with the relevant
    /// topology set. The topology will not be modified in this case, and a
    /// [`ParameterError`] will be returned instead.
    ///
    /// # Aborts
    ///
    /// Failure to allocate internal data will lead to a process abort, because
    /// the topology gets corrupted in this case and must not be touched again,
    /// but we have no way to prevent this in a safe API.
    #[allow(clippy::print_stderr)]
    #[doc(alias = "hwloc_topology_restrict")]
    pub fn restrict<Set: SpecializedBitmap>(
        &mut self,
        set: &Set,
        flags: RestrictFlags,
    ) -> Result<(), ParameterError<Set::Owned>> {
        /// Polymorphized version of this function (avoids generics code bloat)
        fn polymorphized<OwnedSet: OwnedSpecializedBitmap>(
            self_: &mut TopologyEditor<'_>,
            set: &OwnedSet,
            mut flags: RestrictFlags,
        ) -> Result<(), ParameterError<OwnedSet>> {
            // Check if applying this restriction would remove all CPUs/nodes
            //
            // This duplicates some error handling logic inside of hwloc, but
            // reduces the odds that in the presence of errno reporting issues
            // on Windows, the process will abort when it shouldn't.
            let topology = self_.topology();
            let erased_set: &Bitmap = set.as_ref();
            let (affected, other) = match OwnedSet::BITMAP_KIND {
                BitmapKind::CpuSet => {
                    let topology_set = topology.cpuset();
                    let topology_set: &Bitmap = topology_set.as_ref();
                    let cpuset = CpuSet::from(erased_set & topology_set);
                    let nodeset = NodeSet::from_cpuset(topology, &cpuset);
                    (Bitmap::from(cpuset), Bitmap::from(nodeset))
                }
                BitmapKind::NodeSet => {
                    let topology_set = topology.nodeset();
                    let topology_set: &Bitmap = topology_set.as_ref();
                    let nodeset = NodeSet::from(erased_set & topology_set);
                    let cpuset = CpuSet::from_nodeset(topology, &nodeset);
                    (Bitmap::from(nodeset), Bitmap::from(cpuset))
                }
            };
            if affected.is_empty()
                && (flags.contains(RestrictFlags::REMOVE_EMPTIED) || other.is_empty())
            {
                return Err(ParameterError::from(set.to_owned()));
            }

            // Configure restrict flags correctly depending on the node set type
            match OwnedSet::BITMAP_KIND {
                BitmapKind::CpuSet => flags.remove(RestrictFlags::BY_NODE_SET),
                BitmapKind::NodeSet => flags.insert(RestrictFlags::BY_NODE_SET),
            }
            flags.remove(RestrictFlags::REMOVE_CPULESS | RestrictFlags::REMOVE_MEMLESS);
            if flags.contains(RestrictFlags::REMOVE_EMPTIED) {
                flags.remove(RestrictFlags::REMOVE_EMPTIED);
                match OwnedSet::BITMAP_KIND {
                    BitmapKind::CpuSet => {
                        flags.insert(RestrictFlags::REMOVE_CPULESS);
                    }
                    BitmapKind::NodeSet => {
                        flags.insert(RestrictFlags::REMOVE_MEMLESS);
                    }
                }
            }

            // Apply requested restriction
            // SAFETY: - Topology is trusted to contain a valid ptr (type invariant)
            //         - hwloc ops are trusted to keep *mut parameters in a
            //           valid state unless stated otherwise
            //         - set trusted to be valid (Bitmap type invariant)
            //         - hwloc ops are trusted not to modify *const parameters
            //         - By construction, only allowed flag combinations may be sent
            //           to hwloc
            let result = errors::call_hwloc_int_normal("hwloc_topology_restrict", || unsafe {
                hwlocality_sys::hwloc_topology_restrict(
                    self_.topology_mut_ptr(),
                    set.as_ref().as_ptr(),
                    flags.bits(),
                )
            });
            let handle_enomem = |certain: bool| {
                let nuance = if certain { "is" } else { "might be" };
                eprintln!("ERROR: Topology {nuance} stuck in an invalid state. Must abort...");
                std::process::abort()
            };
            match result {
                Ok(_) => Ok(()),
                Err(
                    raw_err @ RawHwlocError {
                        errno: Some(errno), ..
                    },
                ) => match errno.0 {
                    EINVAL => Err(ParameterError::from(set.to_owned())),
                    ENOMEM => handle_enomem(true),
                    _ => unreachable!("Unexpected hwloc error: {raw_err}"),
                },
                Err(raw_err @ RawHwlocError { errno: None, .. }) => {
                    if cfg!(windows) {
                        // Due to errno propagation issues on windows, we may not
                        // know which of EINVAL and ENOMEM we're dealing with. Since
                        // not aborting on ENOMEM is unsafe, we must take the
                        // pessimistic assumption that it was ENOMEM and abort...
                        handle_enomem(false)
                    } else {
                        unreachable!("Unexpected hwloc error: {raw_err}")
                    }
                }
            }
        }
        polymorphized(self, set.borrow(), flags)
    }

    /// Change the sets of allowed PUs and NUMA nodes in the topology
    ///
    /// This function only works if [`BuildFlags::INCLUDE_DISALLOWED`] was set
    /// during topology building. It does not modify any object, it only changes
    /// the sets returned by [`Topology::allowed_cpuset()`] and
    /// [`Topology::allowed_nodeset()`].
    ///
    /// It is notably useful when importing a topology from another process
    /// running in a different Linux Cgroup.
    ///
    /// Removing objects from a topology should rather be performed with
    /// [`TopologyEditor::restrict()`].
    ///
    /// # Errors
    ///
    /// - [`AllowSetError`] if an `AllowSet::Custom` contains neither a cpuset
    ///   nor a nodeset, or if it would remove either all CPUs or all NUMA nodes
    ///   from the allowed set of the topology.
    #[doc(alias = "hwloc_topology_allow")]
    pub fn allow(&mut self, allow_set: AllowSet<'_>) -> Result<(), HybridError<AllowSetError>> {
        // Convert AllowSet into a valid `hwloc_topology_allow` configuration
        let (cpuset, nodeset, flags) = match allow_set {
            AllowSet::All => (ptr::null(), ptr::null(), HWLOC_ALLOW_FLAG_ALL),
            AllowSet::LocalRestrictions => (
                ptr::null(),
                ptr::null(),
                HWLOC_ALLOW_FLAG_LOCAL_RESTRICTIONS,
            ),
            AllowSet::Custom { cpuset, nodeset } => {
                // Check that this operation does not empty any allow-set
                let topology = self.topology();
                let mut effective_cpuset = topology.cpuset().clone_target();
                let mut effective_nodeset = topology.nodeset().clone_target();
                if let Some(cpuset) = cpuset {
                    effective_cpuset &= cpuset;
                    effective_nodeset &= NodeSet::from_cpuset(topology, cpuset);
                }
                if let Some(nodeset) = nodeset {
                    effective_nodeset &= nodeset;
                    effective_cpuset &= CpuSet::from_nodeset(topology, nodeset);
                }
                if effective_cpuset.is_empty() && effective_nodeset.is_empty() {
                    return Err(AllowSetError.into());
                }

                // Check that both sets have been specified
                let cpuset = cpuset.map_or(ptr::null(), CpuSet::as_ptr);
                let nodeset = nodeset.map_or(ptr::null(), NodeSet::as_ptr);
                if cpuset.is_null() && nodeset.is_null() {
                    return Err(AllowSetError.into());
                }
                (cpuset, nodeset, HWLOC_ALLOW_FLAG_CUSTOM)
            }
        };

        // Call hwloc
        // SAFETY: - Topology is trusted to contain a valid ptr (type invariant)
        //         - hwloc ops are trusted to keep *mut parameters in a
        //           valid state unless stated otherwise
        //         - cpusets and nodesets are trusted to be valid (type invariant)
        //         - hwloc ops are trusted not to modify *const parameters
        //         - By construction, flags are trusted to be in sync with the
        //           cpuset and nodeset params + only one of them is set as
        //           requested by hwloc
        errors::call_hwloc_int_normal("hwloc_topology_allow", || unsafe {
            hwlocality_sys::hwloc_topology_allow(self.topology_mut_ptr(), cpuset, nodeset, flags)
        })
        .map(std::mem::drop)
        .map_err(HybridError::Hwloc)
    }

    /// Add more structure to the topology by adding an intermediate [`Group`]
    ///
    /// Use the `find_children` callback to specify which [`TopologyObject`]s of
    /// this topology should be made children of the newly created Group
    /// object. The cpuset and nodeset of the final Group object will be the
    /// union of the cpuset and nodeset of all children respectively. Empty
    /// groups are not allowed, so at least one of these sets must be
    /// non-empty, or no Group object will be created.
    ///
    /// Use the `merge` option to control hwloc's propension to merge groups
    /// with hierarchically-identical topology objects.
    ///
    /// After a successful insertion,
    #[cfg_attr(windows, doc = "[`TopologyObject::set_subtype_unchecked()`]")]
    #[cfg_attr(not(windows), doc = "[`TopologyObject::set_subtype()`]")]
    /// can be used to display something other
    /// than "Group" as the type name for this object in `lstopo`, and custom
    /// name/value info pairs may be added using [`TopologyObject::add_info()`].
    ///
    /// # Errors
    ///
    /// - [`ForeignObjectError`] if some of the child `&TopologyObject`s specified
    ///   by the `find_children` callback do not belong to this [`Topology`].
    /// - [`RawHwlocError`]s are documented to happen if...
    ///     - There are conflicting sets in the topology tree
    ///     - [`Group`] objects are filtered out of the topology through
    ///       [`TypeFilter::KeepNone`]
    ///     - The effective CPU set or NUMA node set ends up being empty.
    ///
    /// [`Group`]: ObjectType::Group
    //
    // --- Implementation details ---
    //
    // In the future, find_children will be an impl FnOnce(&Topology) -> impl
    // IntoIterator<Item = &TopologyObject>, but impl Trait inside of impl
    // Trait is not allowed yet.
    #[doc(alias = "hwloc_topology_alloc_group_object")]
    #[doc(alias = "hwloc_obj_add_other_obj_sets")]
    #[doc(alias = "hwloc_topology_insert_group_object")]
    pub fn insert_group_object(
        &mut self,
        merge: Option<GroupMerge>,
        find_children: impl FnOnce(&Topology) -> Vec<&TopologyObject>,
    ) -> Result<InsertedGroup<'topology>, HybridError<ForeignObjectError>> {
        let mut group = AllocatedGroup::new(self).map_err(HybridError::Hwloc)?;
        group.add_children(find_children)?;
        if let Some(merge) = merge {
            group.set_merge_policy(merge);
        }
        group.insert().map_err(HybridError::Hwloc)
    }

    /// Add a [`Misc`] object as a leaf of the topology
    ///
    /// A new [`Misc`] object will be created and inserted into the topology as
    /// a child of the node selected by `find_parent`. It is appended to the
    /// list of existing Misc children, without ever adding any intermediate
    /// hierarchy level. This is useful for annotating the topology without
    /// actually changing the hierarchy.
    ///
    /// `name` is supposed to be unique across all [`Misc`] objects in the
    /// topology. It must not contain any NUL chars. If it contains any other
    /// non-printable characters, then they will be dropped when exporting to
    /// XML.
    ///
    /// The new leaf object will not have any cpuset.
    ///
    /// # Errors
    ///
    /// - [`ForeignParent`] if the parent `&TopologyObject` returned by
    ///   `find_parent` does not belong to this [`Topology`].
    /// - [`NameContainsNul`] if `name` contains NUL chars.
    /// - An unspecified [`RawHwlocError`] if Misc objects are filtered out of
    ///   the topology via [`TypeFilter::KeepNone`].
    ///
    /// [`ForeignParent`]: InsertMiscError::ForeignParent
    /// [`Misc`]: ObjectType::Misc
    /// [`NameContainsNul`]: InsertMiscError::NameContainsNul
    #[doc(alias = "hwloc_topology_insert_misc_object")]
    pub fn insert_misc_object(
        &mut self,
        name: &str,
        find_parent: impl FnOnce(&Topology) -> &TopologyObject,
    ) -> Result<&'topology mut TopologyObject, HybridError<InsertMiscError>> {
        /// Polymorphized version of this function (avoids generics code bloat)
        ///
        /// # Safety
        ///
        /// - `parent` must point to a [`TopologyObject`] that belongs to
        ///   `self_`
        /// - Any `&TopologyObject` that the pointer parent has been generated
        ///   from must be dropped before calling this function: we'll modify
        ///   its target, so reusing it would be UB.
        unsafe fn polymorphized<'topology>(
            self_: &mut TopologyEditor<'topology>,
            name: &str,
            parent: NonNull<TopologyObject>,
        ) -> Result<&'topology mut TopologyObject, HybridError<InsertMiscError>> {
            // Convert object name to a C string
            let name = LibcString::new(name)
                .map_err(|_| HybridError::Rust(InsertMiscError::NameContainsNul))?;

            // Call hwloc entry point
            let mut ptr =
                // SAFETY: - Topology is trusted to contain a valid ptr (type
                //           invariant)
                //         - hwloc ops are trusted to keep *mut parameters in a
                //           valid state unless stated otherwise
                //         - LibcString should yield valid C strings, which
                //           we're not using beyond their intended lifetime
                //         - hwloc ops are trusted not to modify *const
                //           parameters
                //         - Per polymorphized safety constract, parent should
                //           be correct and not be associated with a live &-ref
                errors::call_hwloc_ptr_mut("hwloc_topology_insert_misc_object", || unsafe {
                    hwlocality_sys::hwloc_topology_insert_misc_object(
                        self_.topology_mut_ptr(),
                        parent.as_inner().as_ptr(),
                        name.borrow(),
                    )
                })
                .map_err(HybridError::Hwloc)?;
            // SAFETY: - If hwloc succeeded, the output pointer is assumed to be
            //           valid and to point to a valid object
            //         - Output lifetime is bound to the topology that it comes
            //           from
            Ok(unsafe { ptr.as_mut().as_newtype() })
        }

        // Find parent object
        let parent: NonNull<TopologyObject> = {
            let topology = self.topology();
            let parent = find_parent(topology);
            if !topology.contains(parent) {
                return Err(InsertMiscError::ForeignParent(parent.into()).into());
            }
            parent.into()
        };

        // SAFETY: parent comes from this topology, source ref has been dropped
        unsafe { polymorphized(self, name, parent) }
    }
}

bitflags! {
    /// Flags to be given to [`TopologyEditor::restrict()`]
    #[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
    #[doc(alias = "hwloc_restrict_flags_e")]
    pub struct RestrictFlags: hwloc_restrict_flags_e {
        /// Remove all objects that lost all resources of the target type
        ///
        /// By default, only objects that contain no PU and no memory are
        /// removed. This flag allows you to remove all objects that...
        ///
        /// - Do not have access to any CPU anymore when restricting by CpuSet
        /// - Do not have access to any memory anymore when restricting by NodeSet
        //
        // --- Implementation details ---
        //
        // This is a virtual flag that is cleared and mapped into
        // `REMOVE_CPULESS` or `REMOVE_MEMLESS` as appropriate.
        #[doc(alias = "HWLOC_RESTRICT_FLAG_REMOVE_CPULESS")]
        #[doc(alias = "HWLOC_RESTRICT_FLAG_REMOVE_MEMLESS")]
        const REMOVE_EMPTIED = hwloc_restrict_flags_e::MAX;

        /// Remove all objects that became CPU-less
        //
        // --- Implementation details ---
        //
        // This is what `REMOVE_EMPTIED` maps into when restricting by `CpuSet`.
        #[doc(hidden)]
        const REMOVE_CPULESS = HWLOC_RESTRICT_FLAG_REMOVE_CPULESS;

        /// Restrict by NodeSet insted of by `CpuSet`
        //
        // --- Implementation details ---
        //
        // This flag is automatically set when restricting by `NodeSet`.
        #[doc(hidden)]
        const BY_NODE_SET = HWLOC_RESTRICT_FLAG_BYNODESET;

        /// Remove all objects that became memory-less
        //
        // --- Implementation details ---
        //
        // This is what `REMOVE_EMPTIED` maps into when restricting by `NodeSet`.
        #[doc(hidden)]
        const REMOVE_MEMLESS = HWLOC_RESTRICT_FLAG_REMOVE_MEMLESS;

        /// Move Misc objects to ancestors if their parents are removed during
        /// restriction
        ///
        /// If this flag is not set, Misc objects are removed when their parents
        /// are removed.
        #[doc(alias = "HWLOC_RESTRICT_FLAG_ADAPT_MISC")]
        const ADAPT_MISC = HWLOC_RESTRICT_FLAG_ADAPT_MISC;

        /// Move I/O objects to ancestors if their parents are removed
        /// during restriction
        ///
        /// If this flag is not set, I/O devices and bridges are removed when
        /// their parents are removed.
        #[doc(alias = "HWLOC_RESTRICT_FLAG_ADAPT_IO")]
        const ADAPT_IO = HWLOC_RESTRICT_FLAG_ADAPT_IO;
    }
}
//
crate::impl_arbitrary_for_bitflags!(RestrictFlags, hwloc_restrict_flags_e);

/// Requested adjustment to the allowed set of PUs and NUMA nodes
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[doc(alias = "hwloc_allow_flags_e")]
pub enum AllowSet<'set> {
    /// Mark all objects as allowed in the topology
    #[doc(alias = "HWLOC_ALLOW_FLAG_ALL")]
    All,

    /// Only allow objects that are available to the current process
    ///
    /// Requires [`BuildFlags::ASSUME_THIS_SYSTEM`] so that the set of available
    /// resources can actually be retrieved from the operating system.
    #[doc(alias = "HWLOC_ALLOW_FLAG_LOCAL_RESTRICTIONS")]
    LocalRestrictions,

    /// Allow a custom set of objects
    ///
    /// You should provide at least one of `cpuset` and `nodeset`.
    #[doc(alias = "HWLOC_ALLOW_FLAG_CUSTOM")]
    Custom {
        /// New value of [`Topology::allowed_cpuset()`]
        cpuset: Option<&'set CpuSet>,

        /// New value of [`Topology::allowed_nodeset()`]
        nodeset: Option<&'set NodeSet>,
    },
}
//
impl fmt::Display for AllowSet<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AllowSet::Custom { cpuset, nodeset } => {
                let mut s = String::from("Custom(");
                if let Some(cpuset) = cpuset {
                    write!(s, "{cpuset}")?;
                    if nodeset.is_some() {
                        s.push_str(", ");
                    }
                }
                if let Some(nodeset) = nodeset {
                    write!(s, "{nodeset}")?;
                }
                s.push(')');
                f.pad(&s)
            }
            other @ (AllowSet::All | AllowSet::LocalRestrictions) => {
                <Self as fmt::Debug>::fmt(other, f)
            }
        }
    }
}
//
impl<'set> From<&'set CpuSet> for AllowSet<'set> {
    fn from(set: &'set CpuSet) -> Self {
        Self::Custom {
            cpuset: Some(set),
            nodeset: None,
        }
    }
}
//
impl<'set> From<&'set NodeSet> for AllowSet<'set> {
    fn from(set: &'set NodeSet) -> Self {
        Self::Custom {
            cpuset: None,
            nodeset: Some(set),
        }
    }
}

/// Attempted to change the allowed set of PUs and NUMA nodes without saying how
#[derive(Copy, Clone, Debug, Default, Eq, Error, Hash, PartialEq)]
#[error("AllowSet::Custom cannot have both empty cpuset AND nodeset members")]
pub struct AllowSetError;

/// Control merging of newly inserted groups with existing objects
#[derive(Copy, Clone, Debug, Display, Eq, Hash, PartialEq, Sequence)]
pub enum GroupMerge {
    /// Prevent the hwloc core from ever merging this Group with another
    /// hierarchically-identical object
    ///
    /// This is useful when the Group itself describes an important feature that
    /// cannot be exposed anywhere else in the hierarchy.
    #[doc(alias = "hwloc_group_attr_s::dont_merge")]
    #[doc(alias = "hwloc_obj_attr_u::hwloc_group_attr_s::dont_merge")]
    Never,

    /// Always discard this new group in favor of any existing Group with the
    /// same locality
    #[doc(alias = "hwloc_group_attr_s::kind")]
    #[doc(alias = "hwloc_obj_attr_u::hwloc_group_attr_s::kind")]
    Always,
}
//
crate::impl_arbitrary_for_sequence!(GroupMerge);
//
impl From<bool> for GroupMerge {
    fn from(value: bool) -> Self {
        if value {
            Self::Always
        } else {
            Self::Never
        }
    }
}

/// RAII guard for `Group` objects that have been allocated, but not inserted
///
/// Ensures that these groups are auto-deleted if not inserted for any reason
/// (typically as a result of erroring out).
///
/// # Safety
///
/// `group` must be a newly allocated, not-yet-inserted `Group` object that is
/// bound to topology editor `editor`. It would be an `&mut TopologyObject` if
/// this didn't break the Rust aliasing rules.
struct AllocatedGroup<'editor, 'topology> {
    /// Group object
    group: NonNull<TopologyObject>,

    /// Underlying TopologyEditor the Group is allocated from
    editor: &'editor mut TopologyEditor<'topology>,
}
//
impl<'editor, 'topology> AllocatedGroup<'editor, 'topology> {
    /// Allocate a new Group object
    pub(self) fn new(
        editor: &'editor mut TopologyEditor<'topology>,
    ) -> Result<Self, RawHwlocError> {
        // SAFETY: - Topology is trusted to contain a valid ptr (type invariant)
        //         - hwloc ops are trusted to keep *mut parameters in a
        //           valid state unless stated otherwise
        errors::call_hwloc_ptr_mut("hwloc_topology_alloc_group_object", || unsafe {
            hwlocality_sys::hwloc_topology_alloc_group_object(editor.topology_mut_ptr())
        })
        .map(|group| Self {
            // SAFETY: - hwloc is trusted to produce a valid, non-inserted group
            //           object pointer
            //         - AsNewtype is trusted to be implemented correctly
            group: unsafe { group.as_newtype() },
            editor,
        })
    }

    /// Expand cpu sets and node sets to cover designated children
    ///
    /// # Errors
    ///
    /// [`ForeignObjectError`] if some of the designated children do not come from
    /// the same topology as this group.
    pub(self) fn add_children(
        &mut self,
        find_children: impl FnOnce(&Topology) -> Vec<&TopologyObject>,
    ) -> Result<(), ForeignObjectError> {
        /// Polymorphized version of this function (avoids generics code bloat)
        ///
        /// # Safety
        ///
        /// - `group` must point to the inner group of an [`AllocatedGroup`]
        /// - `children` must have been checked to belong to the topology of
        ///   said [`AllocatedGroup`]
        unsafe fn polymorphized(group: NonNull<TopologyObject>, children: Vec<&TopologyObject>) {
            // Add children to this group
            for child in children {
                let result =
                    // SAFETY: - group is assumed to be valid as a type
                    //           invariant of AllocatedGroup
                    //         - hwloc ops are trusted not to modify *const
                    //           parameters
                    //         - child was checked to belong to the same
                    //           topology as group
                    //         - AsInner is trusted to be implemented correctly
                    errors::call_hwloc_int_normal("hwloc_obj_add_other_obj_sets", || unsafe {
                        hwlocality_sys::hwloc_obj_add_other_obj_sets(
                            group.as_inner().as_ptr(),
                            child.as_inner(),
                        )
                    });
                let handle_enomem =
                    |raw_err: RawHwlocError| panic!("Internal reallocation failed: {raw_err}");
                match result {
                    Ok(_) => {}
                    Err(
                        raw_err @ RawHwlocError {
                            errno: Some(errno::Errno(ENOMEM)),
                            ..
                        },
                    ) => handle_enomem(raw_err),
                    #[cfg(windows)]
                    Err(raw_err @ RawHwlocError { errno: None, .. }) => {
                        // As explained in the RawHwlocError documentation,
                        // errno values may not correctly propagate from hwloc
                        // to hwlocality on Windows. Since there is only one
                        // expected errno value here, we'll interpret lack of
                        // errno as ENOMEM on Windows.
                        handle_enomem(raw_err)
                    }
                    Err(raw_err) => unreachable!("Unexpected hwloc error: {raw_err}"),
                }
            }
        }

        // Enumerate children, check they belong to this topology
        let topology = self.editor.topology();
        let children = find_children(topology);
        for child in children.iter().copied() {
            if !topology.contains(child) {
                return Err(child.into());
            }
        }

        // Call into the polymorphized function
        // SAFETY: - This is indeed the inner group of this AllocatedGroup
        //         - children have been checked to belong to its topology
        unsafe { polymorphized(self.group, children) };
        Ok(())
    }

    /// Configure hwloc's group merging policy
    ///
    /// By default, hwloc may or may not merge identical groups covering the
    /// same objects. You can encourage or inhibit this tendency with this method.
    pub(self) fn set_merge_policy(&mut self, merge: GroupMerge) {
        let group_attributes: &mut GroupAttributes =
            // SAFETY: - We know this is a group object as a type invariant, so
            //           accessing the group raw attribute is safe
            //         - We trust hwloc to have initialized the group attributes
            //           to a valid state
            //         - We are not changing the raw attributes variant
            unsafe { (&mut (*self.group.as_mut().as_inner().attr).group).as_newtype() };
        match merge {
            GroupMerge::Never => group_attributes.prevent_merging(),
            GroupMerge::Always => group_attributes.favor_merging(),
        }
    }

    /// Insert this Group object into the underlying topology
    ///
    /// # Errors
    ///
    /// Will return an unspecified error if any of the following happens:
    ///
    /// - Insertion failed because of conflicting sets in the topology tree
    /// - Group objects are filtered out of the topology via
    ///   [`TypeFilter::KeepNone`]
    /// - The object was discarded because no set was initialized in the Group,
    ///   or they were all empty.
    pub(self) fn insert(mut self) -> Result<InsertedGroup<'topology>, RawHwlocError> {
        // SAFETY: self is forgotten after this, so no drop or reuse will occur
        let res = unsafe { self.insert_impl() };
        std::mem::forget(self);
        res
    }

    /// Implementation of `insert()` with an `&mut self` argument
    ///
    /// # Errors
    ///
    /// Will return an unspecified error if any of the following happens:
    ///
    /// - Insertion failed because of conflicting sets in the topology tree
    /// - Group objects are filtered out of the topology via
    ///   [`TypeFilter::KeepNone`]
    /// - The object was discarded because no set was initialized in the Group,
    ///   or they were all empty.
    ///
    /// # Safety
    ///
    /// After calling this method, `self` is in an invalid state and should not
    /// be used in any way anymore. In particular, care should be taken to
    /// ensure that its Drop destructor is not called.
    unsafe fn insert_impl(&mut self) -> Result<InsertedGroup<'topology>, RawHwlocError> {
        // SAFETY: - Topology is trusted to contain a valid ptr (type invariant)
        //         - Inner group pointer is assumed valid as a type invariant
        //         - hwloc ops are trusted not to modify *const parameters
        //         - hwloc ops are trusted to keep *mut parameters in a
        //           valid state unless stated otherwise
        //         - We break the AllocatedGroup type invariant by inserting the
        //           group object, but a precondition warns the user about it
        //         - AsInner is trusted to be implemented correctly
        errors::call_hwloc_ptr_mut("hwloc_topology_insert_group_object", || unsafe {
            hwlocality_sys::hwloc_topology_insert_group_object(
                self.editor.topology_mut_ptr(),
                self.group.as_inner().as_ptr(),
            )
        })
        .map(|mut result| {
            if result == self.group.as_inner() {
                // SAFETY: - We know this is a group object as a type invariant
                //         - Output lifetime is bound to the topology it comes from
                //         - Group has been successfully inserted, can expose &mut
                InsertedGroup::New(unsafe { self.group.as_mut() })
            } else {
                // SAFETY: - Successful result is trusted to point to an
                //           existing group, in a valid state
                //         - Output lifetime is bound to the topology it comes from
                InsertedGroup::Existing(unsafe { result.as_mut().as_newtype() })
            }
        })
    }
}
//
impl Drop for AllocatedGroup<'_, '_> {
    #[allow(clippy::print_stderr)]
    fn drop(&mut self) {
        // FIXME: As of hwloc v2.9.4, there is no API to delete a previously
        //        allocated Group object without attempting to insert it into
        //        the topology. An always-failing insertion is the officially
        //        recommended workaround until such an API is added:
        //        https://github.com/open-mpi/hwloc/issues/619
        // SAFETY: - Inner group pointer is assumed valid as a type invariant
        //         - The state where this invariant is invalidated, produced by
        //           insert_impl(), is never exposed to Drop
        unsafe {
            TopologyObject::delete_all_sets(self.group);
        }
        // SAFETY: - AllocatedGroup will not be droppable again after Drop
        if unsafe { self.insert_impl().is_ok() } {
            eprintln!("ERROR: Failed to deallocate group object.");
        }
    }
}

/// Result of inserting a Group object
#[derive(Debug)]
#[must_use]
pub enum InsertedGroup<'topology> {
    /// New Group that was properly inserted
    New(&'topology mut TopologyObject),

    /// Existing object that already fulfilled the role of the proposed Group
    ///
    /// If the Group adds no hierarchy information, hwloc may merge or discard
    /// it in favor of existing topology object at the same location.
    Existing(&'topology mut TopologyObject),
}

/// Error returned by [`TopologyEditor::insert_misc_object()`]
#[derive(Clone, Debug, Eq, Error, Hash, PartialEq)]
pub enum InsertMiscError {
    /// Specified parent does not belong to this topology
    #[error("Misc object parent {0}")]
    ForeignParent(#[from] ForeignObjectError),

    /// Object name contains NUL chars, which hwloc can't handle
    #[error("Misc object name can't contain NUL chars")]
    NameContainsNul,
}
//
impl From<NulError> for InsertMiscError {
    fn from(_: NulError) -> Self {
        Self::NameContainsNul
    }
}

// NOTE: Do not implement traits like AsRef/Deref/Borrow for TopologyEditor,
//       that would be unsafe as it would expose &Topology with unevaluated lazy
//       hwloc caches, and calling their methods could violates Rust's aliasing
//       model via mutation through &Topology.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bitmap::{Bitmap, BitmapRef, OwnedSpecializedBitmap},
        object::{depth::Depth, types::ObjectType, TopologyObjectID},
        strategies::topology_related_set,
    };
    use proptest::prelude::*;
    use similar_asserts::assert_eq;
    use std::{
        collections::{BTreeMap, HashMap},
        ffi::CStr,
        panic::RefUnwindSafe,
    };

    /// Make sure opening/closing the editor doesn't affect the topology
    #[test]
    fn basic_lifecycle() {
        let reference = Topology::test_instance();
        let mut topology = reference.clone();
        topology.edit(|editor| {
            assert_eq!(editor.topology(), reference);
        });
        assert_eq!(&topology, reference);
    }

    // --- Test topology restrictions ---

    proptest! {
        #[test]
        fn restrict_cpuset(
            cpuset in topology_related_set(Topology::cpuset),
            flags: RestrictFlags,
        ) {
            check_restrict(Topology::test_instance(), &cpuset, flags)?;
        }

        #[test]
        fn restrict_nodeset(
            nodeset in topology_related_set(Topology::nodeset),
            flags: RestrictFlags,
        ) {
            check_restrict(Topology::test_instance(), &nodeset, flags)?;
        }
    }

    /// Set-generic test for [`TopologyEditor::restrict()`]
    fn check_restrict<Set: OwnedSpecializedBitmap + RefUnwindSafe>(
        initial_topology: &Topology,
        restrict_set: &Set,
        flags: RestrictFlags,
    ) -> Result<(), TestCaseError> {
        // Compute the restricted topology
        let mut final_topology = initial_topology.clone();
        let result = final_topology.edit(|editor| editor.restrict(restrict_set, flags));

        // Abstract over the kind of set that is being restricted
        let topology_sets = |topology| ErasedSets::from_topology::<Set>(topology);
        let object_sets = |obj: &TopologyObject| ErasedSets::from_object::<Set>(obj);
        let predict_final_sets = |initial_sets: &ErasedSets| {
            initial_sets.predict_restricted(initial_topology, restrict_set)
        };

        // Predict the effect of topology restriction
        let initial_sets = topology_sets(initial_topology);
        let predicted_sets = predict_final_sets(&initial_sets);

        // If one attempts to remove all CPUs and NUMA nodes, and error will be
        // returned and the topology will be unchanged
        if predicted_sets.target.is_empty() {
            prop_assert_eq!(result, Err(ParameterError::from(restrict_set.clone())));
            prop_assert_eq!(initial_topology, &final_topology);
            return Ok(());
        }
        result.unwrap();

        // Otherwise, the topology sets should be restricted as directed
        let final_sets = topology_sets(&final_topology);
        prop_assert_eq!(&final_sets, &predicted_sets);

        // Removing no CPU or node leaves the topology unchanged
        if final_sets == initial_sets {
            prop_assert_eq!(initial_topology, &final_topology);
            return Ok(());
        }

        // Now we're going to predict the outcome on topology objects
        let parent_id =
            |obj: &TopologyObject| obj.parent().map(TopologyObject::global_persistent_index);
        let predict_object =
            |obj: &TopologyObject, predicted_parent_id: Option<TopologyObjectID>| {
                PredictedObject::new(
                    obj,
                    predicted_parent_id,
                    object_sets(obj).map(|sets| predict_final_sets(&sets)),
                )
            };
        let mut predicted_objects = BTreeMap::new();

        // First predict the set of normal and memory objects. Start by
        // including or excluding leaf PU and NUMA node objects...
        let id = |obj: &TopologyObject| obj.global_persistent_index();
        let mut retained_leaves = initial_topology
            .objects_with_type(ObjectType::PU)
            .chain(initial_topology.objects_at_depth(Depth::NUMANode))
            .filter(|obj| {
                let predicted_sets = predict_final_sets(&object_sets(obj).unwrap());
                !(predicted_sets.target.is_empty()
                    && (predicted_sets.other.is_empty()
                        || flags.contains(RestrictFlags::REMOVE_EMPTIED)))
            })
            .map(|obj| (id(obj), obj))
            .collect::<HashMap<_, _>>();

        // ...then recurse into parents to cover the object tree
        let mut next_leaves = HashMap::new();
        while !retained_leaves.is_empty() {
            for (obj_id, obj) in retained_leaves.drain() {
                predicted_objects.insert(obj_id, predict_object(obj, parent_id(obj)));
                if let Some(parent) = obj.parent() {
                    next_leaves.insert(id(parent), parent);
                }
            }
            std::mem::swap(&mut retained_leaves, &mut next_leaves);
        }

        // When their normal parent is destroyed, I/O and Misc objects may
        // either, depending on flags, be deleted or re-attached to the
        // lowest-depth ancestor object that is still present in the topology.
        let rebind_parent = |obj: &TopologyObject| {
            let mut parent = obj.parent().unwrap();
            if !(parent.object_type().is_io() || predicted_objects.contains_key(&id(parent))) {
                parent = parent
                    .ancestors()
                    .find(|ancestor| predicted_objects.contains_key(&id(ancestor)))
                    .unwrap()
            }
            Some(id(parent))
        };

        // Predict the fate I/O objects, including deletions and rebinding
        let io_objects = initial_topology
            .io_objects()
            .filter(|obj| {
                if flags.contains(RestrictFlags::ADAPT_IO) {
                    obj.ancestors()
                        .any(|ancestor| predicted_objects.contains_key(&id(ancestor)))
                } else {
                    predicted_objects.contains_key(&id(obj.first_non_io_ancestor().unwrap()))
                }
            })
            .map(|obj| (id(obj), predict_object(obj, rebind_parent(obj))))
            .collect::<Vec<_>>();

        // Predict the fate of Misc objects using a similar logic
        let misc_objects = initial_topology
            .objects_with_type(ObjectType::Misc)
            .filter(|obj| {
                flags.contains(RestrictFlags::ADAPT_MISC) || {
                    predicted_objects.contains_key(&id(obj.parent().unwrap()))
                }
            })
            .map(|obj| (id(obj), predict_object(obj, rebind_parent(obj))))
            .collect::<Vec<_>>();
        predicted_objects.extend(io_objects);
        predicted_objects.extend(misc_objects);

        // Finally, check that the final object set matches our prediction
        let final_objects = final_topology
            .objects()
            .map(|obj| {
                (
                    id(obj),
                    PredictedObject::new(obj, parent_id(obj), object_sets(obj)),
                )
            })
            .collect::<BTreeMap<_, _>>();
        prop_assert_eq!(predicted_objects, final_objects);
        Ok(())
    }

    /// [`CpuSet`]/[`NodeSet`] abstraction layer
    #[derive(Clone, Debug, Eq, PartialEq)]
    struct ErasedSets {
        /// Set that is being restricted
        target: Bitmap,

        /// Set that is indirectly affected by the restriction
        other: Bitmap,
    }
    //
    impl ErasedSets {
        /// Get [`ErasedSets`] from a [`Topology`]
        fn from_topology<RestrictedSet: OwnedSpecializedBitmap>(topology: &Topology) -> Self {
            match RestrictedSet::BITMAP_KIND {
                BitmapKind::CpuSet => Self {
                    target: Self::ref_to_bitmap(topology.cpuset()),
                    other: Self::ref_to_bitmap(topology.nodeset()),
                },
                BitmapKind::NodeSet => Self {
                    target: Self::ref_to_bitmap(topology.nodeset()),
                    other: Self::ref_to_bitmap(topology.cpuset()),
                },
            }
        }

        /// Get [`ErasedSets`] from a [`TopologyObject`]
        fn from_object<RestrictedSet: OwnedSpecializedBitmap>(
            obj: &TopologyObject,
        ) -> Option<Self> {
            Some(match RestrictedSet::BITMAP_KIND {
                BitmapKind::CpuSet => Self {
                    target: Self::ref_to_bitmap(obj.cpuset()?),
                    other: Self::ref_to_bitmap(obj.nodeset().unwrap()),
                },
                BitmapKind::NodeSet => Self {
                    target: Self::ref_to_bitmap(obj.nodeset()?),
                    other: Self::ref_to_bitmap(obj.cpuset().unwrap()),
                },
            })
        }

        /// Predict the [`ErasedSets`] after restricting the source topology
        fn predict_restricted<RestrictedSet: OwnedSpecializedBitmap>(
            &self,
            initial_topology: &Topology,
            restrict_set: &RestrictedSet,
        ) -> Self {
            let restrict_set: Bitmap = restrict_set.clone().into();
            let predicted_target = &self.target & restrict_set;
            let predicted_other = match RestrictedSet::BITMAP_KIND {
                BitmapKind::CpuSet => {
                    let predicted_target = CpuSet::from(predicted_target.clone());
                    Bitmap::from(NodeSet::from_cpuset(initial_topology, &predicted_target))
                }
                BitmapKind::NodeSet => {
                    let predicted_target = NodeSet::from(predicted_target.clone());
                    Bitmap::from(CpuSet::from_nodeset(initial_topology, &predicted_target))
                }
            };
            Self {
                target: predicted_target,
                other: predicted_other,
            }
        }

        /// Convert a [`BitmapRef`] to a type-erased [`Bitmap`]
        fn ref_to_bitmap<Set: OwnedSpecializedBitmap>(set: BitmapRef<'_, Set>) -> Bitmap {
            set.clone_target().into()
        }
    }

    /// Predicted topology object properties after topology restriction
    #[derive(Clone, Debug, Eq, PartialEq)]
    struct PredictedObject {
        object_type: ObjectType,
        subtype: Option<String>,
        name: Option<String>,
        attributes: Option<String>,
        os_index: Option<usize>,
        depth: Depth,
        parent_id: Option<TopologyObjectID>,
        sets: Option<ErasedSets>,
        infos: String,
    }
    //
    impl PredictedObject {
        /// Given some predicted properties, predict the rest
        fn new(
            obj: &TopologyObject,
            parent_id: Option<TopologyObjectID>,
            sets: Option<ErasedSets>,
        ) -> Self {
            let stringify = |s: Option<&CStr>| s.map(|s| s.to_string_lossy().to_string());
            Self {
                object_type: obj.object_type(),
                subtype: stringify(obj.subtype()),
                name: stringify(obj.name()),
                attributes: obj.attributes().map(|attr| format!("{attr:?}")),
                os_index: obj.os_index(),
                depth: obj.depth(),
                parent_id,
                sets,
                infos: format!("{:?}", obj.infos().iter().collect::<Vec<_>>()),
            }
        }
    }

    // --- Changing the set of allowed PUs and NUMA nodes ---

    /// Owned version of [`AllowSet`]
    #[derive(Clone, Debug, Eq, Hash, PartialEq)]
    enum OwnedAllowSet {
        All,
        LocalRestrictions,
        Custom {
            cpuset: Option<CpuSet>,
            nodeset: Option<NodeSet>,
        },
    }
    //
    impl OwnedAllowSet {
        /// Borrow an [`AllowSet`] from this
        fn as_allow_set(&self) -> AllowSet<'_> {
            match self {
                Self::All => AllowSet::All,
                Self::LocalRestrictions => AllowSet::LocalRestrictions,
                Self::Custom { cpuset, nodeset } => AllowSet::Custom {
                    cpuset: cpuset.as_ref(),
                    nodeset: nodeset.as_ref(),
                },
            }
        }
    }

    /// Generate an `OwnedAllowSet` for `TopologyEditor::allow()` testing
    fn any_allow_set() -> impl Strategy<Value = OwnedAllowSet> {
        fn topology_related_set_opt<Set: OwnedSpecializedBitmap>(
            topology_set: impl FnOnce(&Topology) -> BitmapRef<'_, Set>,
        ) -> impl Strategy<Value = Option<Set>> {
            prop_oneof![
                3 => topology_related_set(topology_set).prop_map(Some),
                2 => Just(None)
            ]
        }
        prop_oneof![
            1 => Just(OwnedAllowSet::All),
            1 => Just(OwnedAllowSet::LocalRestrictions),
            3 => (
                topology_related_set_opt(Topology::complete_cpuset),
                topology_related_set_opt(Topology::complete_nodeset)
            ).prop_map(|(cpuset, nodeset)| OwnedAllowSet::Custom {
                cpuset, nodeset
            })
        ]
    }

    proptest! {
        /// Test [`TopologyEditor::allow()`]
        #[test]
        fn allow(owned_allow_set in any_allow_set()) {
            let initial_topology = Topology::test_instance();
            let mut topology = initial_topology.clone();

            let allow_set = owned_allow_set.as_allow_set();
            let result = topology.edit(|editor| editor.allow(allow_set));

            match allow_set {
                AllowSet::All => {
                    result.unwrap();
                    prop_assert_eq!(topology.allowed_cpuset(), topology.cpuset());
                    prop_assert_eq!(topology.allowed_nodeset(), topology.nodeset());
                }
                AllowSet::LocalRestrictions => {
                    // LocalRestrictions does what the normal topology-building
                    // process does, so it has no observable effect here...
                    result.unwrap();
                    prop_assert_eq!(&topology, initial_topology);
                }
                AllowSet::Custom { cpuset, nodeset } => {
                    if cpuset.is_none() && nodeset.is_none() {
                        prop_assert_eq!(result, Err(AllowSetError.into()));
                        return Ok(());
                    }

                    let mut effective_cpuset = topology.cpuset().clone_target();
                    let mut effective_nodeset = topology.nodeset().clone_target();
                    if let Some(cpuset) = cpuset {
                        effective_cpuset &= cpuset;
                        effective_nodeset &= NodeSet::from_cpuset(&topology, cpuset);
                    }
                    if let Some(nodeset) = nodeset {
                        effective_nodeset &= nodeset;
                        effective_cpuset &= CpuSet::from_nodeset(&topology, nodeset);
                    }
                    if effective_cpuset.is_empty() && effective_nodeset.is_empty() {
                        prop_assert_eq!(result, Err(AllowSetError.into()));
                        return Ok(());
                    }

                    result.unwrap();
                    prop_assert_eq!(topology.allowed_cpuset(), effective_cpuset);
                    prop_assert_eq!(topology.allowed_nodeset(), effective_nodeset);
                }
            }

            // Here we check that LocalRestrictions resets the topology from any
            // allow set we may have configured back to its original allow sets
            let result = topology.edit(|editor| editor.allow(AllowSet::LocalRestrictions));
            result.unwrap();
            prop_assert_eq!(&topology, initial_topology);
        }
    }
}
