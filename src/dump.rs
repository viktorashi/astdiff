use std::path::{Path, PathBuf};
use std::io::{Read, Write};
use anyhow::{Result, bail};
use serde::{Serialize, Deserialize};
use crate::diff::{DiffResult, SerializableDeclaration};

// Magic bytes for the format
const MAGIC_BYTES: &[u8; 4] = b"ASTD";
const CURRENT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Debug)]
pub struct AstDiffDump {
    pub header: DumpHeader,
    pub metadata: DumpMetadata,
    pub file1_data: FileData,
    pub file2_data: FileData,
    pub matching: MatchingData,
    pub diff_result: DiffResult,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DumpHeader {
    pub magic: [u8; 4],
    pub version: u32,
    pub flags: DumpFlags,
    pub checksum: u64,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct DumpFlags {
    pub compressed: bool,
    pub has_source_preview: bool,
    pub has_similarity_matrix: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DumpMetadata {
    pub tool_version: String,
    pub timestamp: u64,
    pub config: DiffConfig,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DiffConfig {
    pub use_fingerprints: bool,
    pub parallel_matching: bool,
    pub threshold: f64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FileData {
    pub path: PathBuf,
    pub content_hash: [u8; 32],
    pub declarations: Vec<DeclarationWithContext>,
    pub source_preview: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DeclarationWithContext {
    pub decl: SerializableDeclaration,
    pub candidates_considered: Vec<(usize, f64)>,
    pub match_decision: Option<MatchDecision>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MatchingData {
    pub matches: Vec<MatchPair>,
    pub similarity_matrix: Option<SparseMatrix>,
    pub threshold_data: ThresholdInfo,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MatchPair {
    pub idx1: usize,
    pub idx2: usize,
    pub similarity: f64,
    pub evidence_count: usize,
    pub evidence_breakdown: Option<EvidenceBreakdown>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MatchDecision {
    pub matched_to: Option<usize>,
    pub similarity_score: f64,
    pub reason: MatchReason,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum MatchReason {
    HighSimilarity { score: f64, evidence: usize },
    FingerprintMatch { common_strings: usize, common_apis: usize },
    NoSuitableCandidate,
    BetterMatchExists { better_idx: usize, better_score: f64 },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SparseMatrix {
    pub entries: Vec<(usize, usize, f64)>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ThresholdInfo {
    pub used_threshold: f64,
    pub computed_threshold: Option<f64>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EvidenceBreakdown {
    pub structural_similarity: f64,
    pub name_similarity: f64,
    pub fingerprint_similarity: Option<f64>,
}

impl AstDiffDump {
    /// Create a new dump from analysis results
    pub fn new(
        file1_path: PathBuf,
        file2_path: PathBuf,
        file1_decls: Vec<SerializableDeclaration>,
        file2_decls: Vec<SerializableDeclaration>,
        matches: Vec<(usize, usize, f64)>,
        diff_result: DiffResult,
        config: DiffConfig,
    ) -> Result<Self> {
        // Calculate content hashes
        let file1_hash = Self::calculate_file_hash(&file1_path)?;
        let file2_hash = Self::calculate_file_hash(&file2_path)?;
        
        // Create file data
        let file1_data = FileData {
            path: file1_path,
            content_hash: file1_hash,
            declarations: file1_decls.into_iter().map(|decl| DeclarationWithContext {
                decl,
                candidates_considered: vec![],
                match_decision: None,
            }).collect(),
            source_preview: None,
        };
        
        let file2_data = FileData {
            path: file2_path,
            content_hash: file2_hash,
            declarations: file2_decls.into_iter().map(|decl| DeclarationWithContext {
                decl,
                candidates_considered: vec![],
                match_decision: None,
            }).collect(),
            source_preview: None,
        };
        
        // Create matching data
        let matching = MatchingData {
            matches: matches.into_iter().map(|(idx1, idx2, sim)| MatchPair {
                idx1,
                idx2,
                similarity: sim,
                evidence_count: 0,
                evidence_breakdown: None,
            }).collect(),
            similarity_matrix: None,
            threshold_data: ThresholdInfo {
                used_threshold: config.threshold,
                computed_threshold: None,
            },
        };
        
        // Create metadata
        let metadata = DumpMetadata {
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            config,
        };
        
        // Create header
        let header = DumpHeader {
            magic: *MAGIC_BYTES,
            version: CURRENT_VERSION,
            flags: DumpFlags {
                compressed: true,
                has_source_preview: false,
                has_similarity_matrix: false,
            },
            checksum: 0, // Will be calculated later
        };
        
        Ok(Self {
            header,
            metadata,
            file1_data,
            file2_data,
            matching,
            diff_result,
        })
    }
    
    /// Save the dump to a file
    pub fn save(&self, path: &Path) -> Result<()> {
        // Serialize the main data
        let data = bincode::serialize(&self)?;
        
        // Compress with zstd
        let compressed = zstd::encode_all(&data[..], 3)?;
        
        // Write to file
        let mut file = std::fs::File::create(path)?;
        file.write_all(&compressed)?;
        
        Ok(())
    }
    
    /// Load a dump from a file
    pub fn load(path: &Path) -> Result<Self> {
        // Read the file
        let mut file = std::fs::File::open(path)?;
        let mut compressed = Vec::new();
        file.read_to_end(&mut compressed)?;
        
        // Decompress
        let data = zstd::decode_all(&compressed[..])?;
        
        // Deserialize
        let dump: Self = bincode::deserialize(&data)?;
        
        // Verify magic and version
        if dump.header.magic != *MAGIC_BYTES {
            bail!("Invalid magic bytes in dump file");
        }
        
        if dump.header.version > CURRENT_VERSION {
            bail!("Dump file version {} is newer than supported version {}", 
                  dump.header.version, CURRENT_VERSION);
        }
        
        Ok(dump)
    }
    
    /// Calculate SHA-256 hash of a file
    fn calculate_file_hash(path: &Path) -> Result<[u8; 32]> {
        use sha2::{Sha256, Digest};
        
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 8192];
        
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        
        Ok(hasher.finalize().into())
    }
    
    /// Find a declaration by name
    pub fn find_declaration(&self, name: &str) -> Option<&DeclarationWithContext> {
        self.file1_data.declarations.iter()
            .chain(self.file2_data.declarations.iter())
            .find(|d| d.decl.name == name)
    }
    
    /// Get the match for a declaration from file1
    pub fn get_match_for(&self, file1_decl_idx: usize) -> Option<&MatchPair> {
        self.matching.matches.iter()
            .find(|m| m.idx1 == file1_decl_idx)
    }
    
    /// Get all unmatched declarations from file1
    pub fn unmatched_from_file1(&self) -> Vec<&DeclarationWithContext> {
        let matched_indices: std::collections::HashSet<_> = 
            self.matching.matches.iter().map(|m| m.idx1).collect();
        
        self.file1_data.declarations.iter()
            .enumerate()
            .filter(|(idx, _)| !matched_indices.contains(idx))
            .map(|(_, decl)| decl)
            .collect()
    }
    
    /// Get all unmatched declarations from file2
    pub fn unmatched_from_file2(&self) -> Vec<&DeclarationWithContext> {
        let matched_indices: std::collections::HashSet<_> = 
            self.matching.matches.iter().map(|m| m.idx2).collect();
        
        self.file2_data.declarations.iter()
            .enumerate()
            .filter(|(idx, _)| !matched_indices.contains(idx))
            .map(|(_, decl)| decl)
            .collect()
    }
    
    /// Validate that the dump is still valid for the given source files
    pub fn validate(&self, file1_path: &Path, file2_path: &Path) -> Result<bool> {
        let file1_hash = Self::calculate_file_hash(file1_path)?;
        let file2_hash = Self::calculate_file_hash(file2_path)?;
        
        Ok(file1_hash == self.file1_data.content_hash && 
           file2_hash == self.file2_data.content_hash)
    }
}