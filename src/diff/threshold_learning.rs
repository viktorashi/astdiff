use std::collections::HashMap;
use serde::{Serialize, Deserialize};

/// Training data for threshold learning
#[derive(Debug, Serialize, Deserialize)]
pub struct MatchingExample {
    pub file1: String,
    pub file2: String,
    pub func1_name: String,
    pub func2_name: String,
    pub should_match: bool,
    pub confidence: f64,  // How sure we are about this label (0.0-1.0)
    pub notes: Option<String>,
}

/// Learned threshold parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearnedThresholds {
    /// Base threshold for different evidence counts
    pub evidence_thresholds: HashMap<usize, f64>,
    
    /// Multipliers for different contexts
    pub context_weights: HashMap<String, f64>,
    
    /// Size ratio penalties
    pub size_penalties: Vec<(f64, f64)>, // (ratio, penalty)
    
    /// Minimum evidence pieces required
    pub min_evidence: usize,
    
    /// Confidence intervals
    pub confidence_intervals: Vec<(f64, f64, String)>, // (min, max, label)
}

impl Default for LearnedThresholds {
    fn default() -> Self {
        // Conservative defaults
        let mut evidence_thresholds = HashMap::new();
        evidence_thresholds.insert(1, 0.8);  // Single evidence needs high score
        evidence_thresholds.insert(2, 0.6);  // Two pieces can be lower
        evidence_thresholds.insert(3, 0.5);  // Three pieces even lower
        evidence_thresholds.insert(5, 0.4);  // Many pieces = lower threshold
        
        let mut context_weights = HashMap::new();
        context_weights.insert("ErrorMessage".to_string(), 1.2);
        context_weights.insert("FilePath".to_string(), 1.1);
        context_weights.insert("CommandName".to_string(), 1.0);
        context_weights.insert("ConfigKey".to_string(), 0.9);
        context_weights.insert("Regular".to_string(), 0.7);
        
        Self {
            evidence_thresholds,
            context_weights,
            size_penalties: vec![
                (0.3, 0.5),  // < 30% size match = 50% penalty
                (0.5, 0.2),  // < 50% size match = 20% penalty
                (0.7, 0.1),  // < 70% size match = 10% penalty
                (1.0, 0.0),  // >= 70% = no penalty
            ],
            min_evidence: 2,
            confidence_intervals: vec![
                (0.0, 0.3, "no_match".to_string()),
                (0.3, 0.6, "possible_match".to_string()),
                (0.6, 0.8, "likely_match".to_string()),
                (0.8, 1.0, "definite_match".to_string()),
            ],
        }
    }
}

pub struct ThresholdLearner {
    examples: Vec<MatchingExample>,
    current_thresholds: LearnedThresholds,
}

impl ThresholdLearner {
    pub fn new() -> Self {
        Self {
            examples: Vec::new(),
            current_thresholds: LearnedThresholds::default(),
        }
    }
    
    /// Add a training example
    pub fn add_example(&mut self, example: MatchingExample) {
        self.examples.push(example);
    }
    
    /// Learn from labeled examples using a simple grid search
    pub fn learn_thresholds(&mut self) -> LearnedThresholds {
        // Grid search over threshold space
        let evidence_options = vec![
            vec![0.9, 0.7, 0.5, 0.4],  // 1 evidence
            vec![0.7, 0.5, 0.4, 0.3],  // 2 evidence
            vec![0.5, 0.4, 0.3, 0.2],  // 3+ evidence
        ];
        
        let min_evidence_options = vec![1, 2, 3];
        
        let mut best_thresholds = self.current_thresholds.clone();
        let mut best_score = 0.0;
        
        // Try different combinations
        for min_ev in &min_evidence_options {
            for ev1 in &evidence_options[0] {
                for ev2 in &evidence_options[1] {
                    for ev3 in &evidence_options[2] {
                        let mut test_thresholds = best_thresholds.clone();
                        test_thresholds.min_evidence = *min_ev;
                        test_thresholds.evidence_thresholds.insert(1, *ev1);
                        test_thresholds.evidence_thresholds.insert(2, *ev2);
                        test_thresholds.evidence_thresholds.insert(3, *ev3);
                        
                        let score = self.evaluate_thresholds(&test_thresholds);
                        if score > best_score {
                            best_score = score;
                            best_thresholds = test_thresholds;
                        }
                    }
                }
            }
        }
        
        self.current_thresholds = best_thresholds.clone();
        best_thresholds
    }
    
    /// Evaluate how well thresholds perform on training data
    fn evaluate_thresholds(&self, thresholds: &LearnedThresholds) -> f64 {
        let mut correct = 0;
        let mut total = 0;
        
        for example in &self.examples {
            // This would need the actual matching scores from the example
            // For now, simulate based on the example data
            let predicted = self.predict_match(example, thresholds);
            
            if predicted == example.should_match {
                correct += 1;
            }
            total += 1;
        }
        
        correct as f64 / total as f64
    }
    
    fn predict_match(&self, example: &MatchingExample, _thresholds: &LearnedThresholds) -> bool {
        // Simplified prediction - would need actual fingerprint data
        // For now, use name similarity as a proxy
        let name_sim = self.name_similarity(&example.func1_name, &example.func2_name);
        name_sim > 0.5
    }
    
    fn name_similarity(&self, n1: &str, n2: &str) -> f64 {
        if n1 == n2 { return 1.0; }
        let len_diff = (n1.len() as f64 - n2.len() as f64).abs();
        let max_len = n1.len().max(n2.len()) as f64;
        1.0 - (len_diff / max_len)
    }
    
    /// Save learned thresholds to a file
    pub fn save_thresholds(&self, path: &str) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(&self.current_thresholds)?;
        std::fs::write(path, json)
    }
    
    /// Load thresholds from a file
    pub fn load_thresholds(&mut self, path: &str) -> std::io::Result<()> {
        let json = std::fs::read_to_string(path)?;
        self.current_thresholds = serde_json::from_str(&json)?;
        Ok(())
    }
}

/// Active learning - suggest examples that would be most helpful
pub struct ActiveLearner {
    uncertainty_threshold: f64,
}

impl ActiveLearner {
    pub fn new() -> Self {
        Self {
            uncertainty_threshold: 0.2, // Within 20% of decision boundary
        }
    }
    
    /// Find function pairs where we're uncertain about matching
    pub fn suggest_labeling_candidates(&self, 
        scores: Vec<(String, String, f64, usize)>, // (func1, func2, score, evidence_count)
        current_threshold: f64,
    ) -> Vec<(String, String, f64)> {
        let mut candidates = Vec::new();
        
        for (f1, f2, score, _) in scores {
            let distance_to_threshold = (score - current_threshold).abs();
            if distance_to_threshold < self.uncertainty_threshold {
                candidates.push((f1, f2, score));
            }
        }
        
        // Sort by how close to threshold (most uncertain first)
        candidates.sort_by(|a, b| {
            let dist_a = (a.2 - current_threshold).abs();
            let dist_b = (b.2 - current_threshold).abs();
            dist_a.partial_cmp(&dist_b).unwrap()
        });
        
        candidates
    }
}

/// Generate training data from user corrections
pub struct TrainingDataCollector {
    corrections: Vec<(String, String, bool)>, // (func1, func2, user_said_match)
}

impl TrainingDataCollector {
    pub fn new() -> Self {
        Self {
            corrections: Vec::new(),
        }
    }
    
    /// Record when user corrects a matching decision
    pub fn record_correction(&mut self, func1: &str, func2: &str, should_match: bool) {
        self.corrections.push((func1.to_string(), func2.to_string(), should_match));
    }
    
    /// Convert corrections to training examples
    pub fn to_training_examples(&self, file1: &str, file2: &str) -> Vec<MatchingExample> {
        self.corrections.iter().map(|(f1, f2, should_match)| {
            MatchingExample {
                file1: file1.to_string(),
                file2: file2.to_string(),
                func1_name: f1.clone(),
                func2_name: f2.clone(),
                should_match: *should_match,
                confidence: 0.9, // User corrections are high confidence
                notes: Some("User correction".to_string()),
            }
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_threshold_learning() {
        let mut learner = ThresholdLearner::new();
        
        // Add some examples
        learner.add_example(MatchingExample {
            file1: "v1.js".to_string(),
            file2: "v2.js".to_string(),
            func1_name: "D4Q".to_string(),
            func2_name: "vK1".to_string(),
            should_match: true,
            confidence: 0.8,
            notes: Some("Same function, renamed".to_string()),
        });
        
        learner.add_example(MatchingExample {
            file1: "v1.js".to_string(),
            file2: "v2.js".to_string(),
            func1_name: "handleError".to_string(),
            func2_name: "processRequest".to_string(),
            should_match: false,
            confidence: 0.9,
            notes: Some("Different functions".to_string()),
        });
        
        let thresholds = learner.learn_thresholds();
        assert!(thresholds.min_evidence > 0);
    }
}