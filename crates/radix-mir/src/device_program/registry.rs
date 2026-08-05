use super::types::{BufferId, BufferIdentity, BufferVersion, LaunchId};

/// The derived registry of every buffer, version, and data-flow edge in a
/// device program (see [`DeviceProgram::buffer_registry`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferRegistry {
    /// One entry per distinct buffer id, in first-reference order.
    pub buffers: Vec<RegistryBuffer>,
}

impl BufferRegistry {
    /// Look up a buffer entry by id.
    #[must_use]
    pub fn buffer(&self, id: BufferId) -> Option<&RegistryBuffer> {
        self.buffers.iter().find(|entry| entry.identity.id == id)
    }

    /// Inter-kernel data-flow edges: for every buffer version, the
    /// producer→consumer launch pairs.
    ///
    /// This is the generalized [`InterKernelBufferIdentity`] concept: the WGSL
    /// descriptor maps positional output (kernel, binding) pairs to input
    /// pairs; here an edge is a declared buffer identity + content version,
    /// attributed to the launching that produce and consume it. Self-edges
    /// (a launch that both writes and reads the same version) are excluded —
    /// they are intra-kernel access, not inter-kernel flow.
    #[must_use]
    pub fn data_flow_pairs(&self) -> Vec<DataFlowPair> {
        let mut pairs = Vec::new();
        for entry in &self.buffers {
            for version in &entry.versions {
                for &producer in &version.produced_by {
                    for &consumer in &version.consumed_by {
                        if producer != consumer {
                            pairs.push(DataFlowPair {
                                buffer: entry.identity.id,
                                version: version.version.version,
                                producer,
                                consumer,
                            });
                        }
                    }
                }
            }
        }
        pairs
    }
}

/// One registry entry: the identity facts of a buffer plus its version chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryBuffer {
    /// The buffer's program-level identity facts.
    pub identity: BufferIdentity,
    /// Content versions observed in the program, in first-reference order.
    pub versions: Vec<RegistryVersion>,
}

/// One registry entry: a content version with its producer/consumer launches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryVersion {
    /// The content version and its shape.
    pub version: BufferVersion,
    /// Launches whose kernels write the version (write or read-write slots).
    pub produced_by: Vec<LaunchId>,
    /// Launches whose kernels read the version (read or read-write slots).
    pub consumed_by: Vec<LaunchId>,
}

/// One inter-kernel data-flow edge (see [`BufferRegistry::data_flow_pairs`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DataFlowPair {
    /// Buffer whose content flows between the two launches.
    pub buffer: BufferId,
    /// Content version that flows.
    pub version: u32,
    /// Launch that writes the version.
    pub producer: LaunchId,
    /// Launch that reads the version.
    pub consumer: LaunchId,
}
