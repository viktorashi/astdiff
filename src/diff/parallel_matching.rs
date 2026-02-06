use super::{DeclarationData, DeclarationKind, Change, ChangeType};
use super::fingerprint::{FunctionFingerprint, calculate_fingerprint_similarity, RarityScorer};
use super::matching_report::EvidenceBreakdown;
use rayon::prelude::*;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct CandidateMatch {
    pub i1: usize,
    pub i2: usize,
    pub similarity: f64,
    pub evidence_count: usize,
    pub evidence_breakdown: Option<EvidenceBreakdown>,
}

pub struct MatchingContext<'a> {
    pub decls1: &'a [DeclarationData],
    pub decls2: &'a [DeclarationData],
    pub source1: &'a str,
    pub source2: &'a str,
    pub scorer: Option<&'a RarityScorer>,
    pub use_fingerprints: bool,
}

pub struct ParallelMatcher {}

impl ParallelMatcher {
    pub fn new(_use_fingerprints: bool, _generate_report: bool) -> Self {
        Self {}
    }
    
    /// Perform parallel matching of declarations
    pub fn match_declarations(
        &self,
        context: MatchingContext,
        calculate_similarity: impl Fn(&DeclarationData, &DeclarationData, &str, &str) -> f64 + Sync,
        create_evidence: impl Fn(&DeclarationData, &DeclarationData, &FunctionFingerprint, &FunctionFingerprint, &RarityScorer) -> EvidenceBreakdown + Sync,
    ) -> (Vec<(usize, usize)>, Vec<Change>) {
        let MatchingContext { decls1, decls2, source1, source2, scorer, use_fingerprints } = context;
        
        // Create size-sorted indices for efficient searching
        let mut sorted2_indices: Vec<(usize, usize)> = decls2.iter()
            .enumerate()
            .map(|(i, d)| (i, d.size))
            .collect();
        sorted2_indices.sort_by_key(|(_, size)| *size);
        
        // Phase 1: Parallel candidate generation
        // Split decls1 into chunks for parallel processing
        let candidates = self.generate_candidates_parallel(
            decls1, 
            decls2, 
            &sorted2_indices,
            scorer,
            use_fingerprints,
            &calculate_similarity,
            &create_evidence,
            source1,
            source2,
        );
        
        // Phase 2: Resolve conflicts where multiple decls1 matched the same decl2
        let (matches, changes) = self.resolve_conflicts(
            candidates,
            decls1,
            decls2,
            source1,
            source2,
        );
        
        (matches, changes)
    }
    
    fn generate_candidates_parallel(
        &self,
        decls1: &[DeclarationData],
        decls2: &[DeclarationData],
        sorted2_indices: &[(usize, usize)],
        scorer: Option<&RarityScorer>,
        use_fingerprints: bool,
        calculate_similarity: &(impl Fn(&DeclarationData, &DeclarationData, &str, &str) -> f64 + Sync),
        create_evidence: &(impl Fn(&DeclarationData, &DeclarationData, &FunctionFingerprint, &FunctionFingerprint, &RarityScorer) -> EvidenceBreakdown + Sync),
        source1: &str,
        source2: &str,
    ) -> Vec<CandidateMatch> {
        // Process declarations in parallel
        let all_candidates: Vec<Vec<CandidateMatch>> = (0..decls1.len())
            .into_par_iter()
            .map(|i1| {
                let decl1 = &decls1[i1];
                let mut local_candidates = Vec::new();
                
                // Find size window
                let min_size = ((decl1.size as f64) * 0.5).max(1.0) as usize;
                let max_size = ((decl1.size as f64) * 1.5) as usize;
                
                // Binary search for start of size window
                let start_idx = sorted2_indices.partition_point(|(_, size)| *size < min_size);
                
                // Collect candidates within size window
                for idx in start_idx..sorted2_indices.len() {
                    let (i2, size2) = sorted2_indices[idx];
                    if size2 > max_size {
                        break;
                    }
                    
                    let decl2 = &decls2[i2];
                    if decl1.kind != decl2.kind {
                        continue;
                    }
                    
                    // Quick LSH filter
                    let lsh_sim = estimate_minhash_similarity(
                        &decl1.minhash_signature,
                        &decl2.minhash_signature
                    );
                    
                    if lsh_sim < 0.3 {
                        continue;
                    }
                    
                    // Calculate full similarity
                    let (similarity, evidence_count, evidence_breakdown) = 
                        if use_fingerprints {
                            if let (Some(ref fp1), Some(ref fp2), Some(s)) = 
                                (&decl1.fingerprint, &decl2.fingerprint, scorer) {
                                let (fp_score, ev_count) = calculate_fingerprint_similarity(fp1, fp2, s);
                                let breakdown = create_evidence(decl1, decl2, fp1, fp2, s);
                                let struct_sim = calculate_similarity(decl1, decl2, source1, source2);
                                let combined = fp_score * 0.7 + struct_sim * 0.3;
                                (combined, ev_count, Some(breakdown))
                            } else {
                                (calculate_similarity(decl1, decl2, source1, source2), 0, None)
                            }
                        } else {
                            (calculate_similarity(decl1, decl2, source1, source2), 0, None)
                        };
                    
                    // Check if this is a viable match
                    if should_consider_match(similarity, evidence_count, decl1.size) {
                        local_candidates.push(CandidateMatch {
                            i1,
                            i2,
                            similarity,
                            evidence_count,
                            evidence_breakdown,
                        });
                    }
                }
                
                // Keep only the best matches for this declaration
                local_candidates.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
                local_candidates.truncate(3); // Keep top 3 candidates
                
                local_candidates
            })
            .collect();
        
        // Flatten results
        all_candidates.into_iter().flatten().collect()
    }
    
    fn resolve_conflicts(
        &self,
        candidates: Vec<CandidateMatch>,
        decls1: &[DeclarationData],
        decls2: &[DeclarationData],
        source1: &str,
        source2: &str,
    ) -> (Vec<(usize, usize)>, Vec<Change>) {
        // Group candidates by target (i2)
        let mut candidates_by_target: HashMap<usize, Vec<CandidateMatch>> = HashMap::new();
        for candidate in candidates {
            candidates_by_target.entry(candidate.i2)
                .or_insert_with(Vec::new)
                .push(candidate);
        }
        
        let mut matches = Vec::new();
        let mut changes = Vec::new();
        let mut matched1 = vec![false; decls1.len()];
        let mut matched2 = vec![false; decls2.len()];
        
        // Resolve conflicts by choosing the best match for each target
        for (i2, mut candidates_for_target) in candidates_by_target {
            if matched2[i2] {
                continue;
            }
            
            // Sort by similarity to get the best match
            candidates_for_target.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
            
            // Try to find a match that hasn't been matched yet
            for candidate in candidates_for_target {
                if matched1[candidate.i1] {
                    continue;
                }
                
                // Check if this match meets our criteria
                let should_match = if candidate.evidence_count > 0 {
                    should_match_with_evidence(candidate.similarity, candidate.evidence_count, decls1[candidate.i1].size)
                } else {
                    // For non-fingerprint matching, we need to check against second best
                    // This is simplified for now - in production we'd track second best scores
                    should_match_simple(candidate.similarity, decls1[candidate.i1].size)
                };
                
                if should_match {
                    matched1[candidate.i1] = true;
                    matched2[i2] = true;
                    matches.push((candidate.i1, i2));
                    
                    let decl1 = &decls1[candidate.i1];
                    let decl2 = &decls2[i2];
                    
                    // Generate changes
                    changes.extend(generate_changes_for_match(
                        decl1, decl2, candidate.similarity, source1, source2
                    ));
                    
                    break; // Found a match for this target
                }
            }
        }
        
        // Sort matches by original order
        matches.sort_by_key(|(i1, _)| *i1);
        
        (matches, changes)
    }
}

// Helper functions

fn estimate_minhash_similarity(sig1: &[u64], sig2: &[u64]) -> f64 {
    let matches = sig1.iter().zip(sig2).filter(|(a, b)| a == b).count();
    matches as f64 / sig1.len() as f64
}

fn should_consider_match(similarity: f64, evidence_count: usize, size: usize) -> bool {
    // More lenient thresholds for initial consideration
    if evidence_count > 0 {
        similarity >= 0.3
    } else if size < 10 {
        similarity >= 0.4
    } else {
        similarity >= 0.25
    }
}

fn should_match_with_evidence(score: f64, evidence_count: usize, _size: usize) -> bool {
    match evidence_count {
        0 => false,
        1 => score >= 0.6,
        2 => score >= 0.45,
        3..=4 => score >= 0.4,
        _ => score >= 0.35,
    }
}

fn should_match_simple(similarity: f64, size: usize) -> bool {
    if similarity >= 0.85 {
        return true;
    }
    
    if size < 10 {
        similarity >= 0.7
    } else if size < 50 {
        similarity >= 0.5
    } else {
        similarity >= 0.4
    }
}

fn generate_changes_for_match(
    decl1: &DeclarationData,
    decl2: &DeclarationData,
    similarity: f64,
    source1: &str,
    source2: &str,
) -> Vec<Change> {
    let mut changes = Vec::new();
    
    // Check if names differ
    if decl1.name != decl2.name {
        changes.push(Change {
            change_type: ChangeType::Modification,
            location1: Some(create_location_stub(decl1, source1)),
            location2: Some(create_location_stub(decl2, source2)),
            description: format!("{} '{}' matched with '{}'",
                kind_to_string(&decl1.kind), decl1.name, decl2.name),
            structural_path: format!("global.{}->{}", decl1.name, decl2.name),
            string_diff: None,
        });
    }

    // Check if it's a reorder
    if decl1.line != decl2.line {
        changes.push(Change {
            change_type: ChangeType::Reorder,
            location1: Some(create_location_stub(decl1, source1)),
            location2: Some(create_location_stub(decl2, source2)),
            description: format!("{} '{}' moved from line {} to line {}",
                kind_to_string(&decl1.kind), decl1.name, decl1.line, decl2.line),
            structural_path: format!("global.{}", decl1.name),
            string_diff: None,
        });
    }

    // Check for signature changes
    if similarity < 0.95 && decl1.signature != decl2.signature {
        changes.push(Change {
            change_type: ChangeType::Modification,
            location1: Some(create_location_stub(decl1, source1)),
            location2: Some(create_location_stub(decl2, source2)),
            description: format!("{} '{}' structure changed (similarity: {:.1}%)",
                kind_to_string(&decl1.kind), decl1.name, similarity * 100.0),
            structural_path: format!("global.{}", decl1.name),
            string_diff: None,
        });
    }
    
    changes
}

pub(crate) fn create_location_stub(decl: &DeclarationData, source: &str) -> super::Location {
    // Extract a code snippet from the source
    let lines: Vec<&str> = source.lines().collect();
    let snippet = if decl.line > 0 && decl.line <= lines.len() {
        lines[decl.line - 1].trim().to_string()
    } else {
        String::new()
    };
    
    super::Location {
        line: decl.line,
        column: 0,
        code_snippet: snippet,
        end_line: Some(decl.end_line),
    }
}

fn kind_to_string(kind: &DeclarationKind) -> &'static str {
    match kind {
        DeclarationKind::Function => "function",
        DeclarationKind::Class => "class",
        DeclarationKind::Variable => "variable",
        DeclarationKind::Import => "import",
        DeclarationKind::Export => "export",
    }
}