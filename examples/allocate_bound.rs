use anyhow::Context;
use hwlocality::{
    memory::binding::{MemoryBindingFlags, MemoryBindingPolicy},
    objects::{depth::Depth, TopologyObject},
    Topology,
};

/// Allocate 4 MiB of memory that is bound to the last NUMA node on the system
fn main() -> anyhow::Result<()> {
    // Create topology and check feature support
    let topology = Topology::new()?;
    let Some(support) = topology.feature_support().memory_binding() else {
        println!("This example requires memory binding support");
        return Ok(());
    };
    if !(support.alloc() || support.set_current_process() || support.set_current_thread()) {
        println!(
            "This example needs support for querying and setting current process CPU bindings"
        );
        return Ok(());
    }

    // Find the last NUMA node on the system and
    let nodeset = last_node(&topology)?
        .nodeset()
        .context("NUMA nodes should have nodesets")?;

    // Allocate memory that is bound to this NUMA node, binding ourselves to
    // it if necessary.
    const ALLOC_SIZE: usize = 4 * 1024 * 1024;
    println!("Will now allocate {ALLOC_SIZE} bytes of memory bound to NUMA node {nodeset}");
    let _bytes = topology.binding_allocate_memory(
        ALLOC_SIZE,
        &nodeset,
        MemoryBindingPolicy::default(),
        MemoryBindingFlags::ASSUME_SINGLE_THREAD,
    )?;

    Ok(())
}

/// Find the last NUMA node one the system
fn last_node(topology: &Topology) -> anyhow::Result<&TopologyObject> {
    let mut all_nodes = topology.objects_at_depth(Depth::NUMANode);
    all_nodes
        .next_back()
        .context("At least one NUMA node should be present")
}