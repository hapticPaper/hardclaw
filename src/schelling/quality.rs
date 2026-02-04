//! Quality assessment for subjective tasks.

use serde::{Deserialize, Serialize};

/// A quality metric for subjective assessment
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityMetric {
    /// General quality (0-100)
    Overall,
    /// Task-specific: creativity
    Creativity,
    /// Task-specific: accuracy
    Accuracy,
    /// Task-specific: coherence
    Coherence,
    /// Task-specific: completeness
    Completeness,
    /// Task-specific: relevance
    Relevance,
    /// Custom metric
    Custom(String),
}

/// Quality assessment for a solution
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QualityAssessment {
    /// Overall quality score (0-100)
    pub overall_score: u8,
    /// Individual metric scores
    pub metrics: Vec<(QualityMetric, u8)>,
    /// Optional textual feedback
    pub feedback: Option<String>,
}

impl QualityAssessment {
    /// Create a simple overall assessment
    #[must_use]
    pub const fn simple(score: u8) -> Self {
        Self {
            overall_score: score,
            metrics: Vec::new(),
            feedback: None,
        }
    }

    /// Create a detailed assessment with multiple metrics
    #[must_use]
    pub fn detailed(metrics: Vec<(QualityMetric, u8)>) -> Self {
        let overall_score = if metrics.is_empty() {
            0
        } else {
            let sum: u32 = metrics.iter().map(|(_, s)| u32::from(*s)).sum();
            (sum / metrics.len() as u32) as u8
        };

        Self {
            overall_score,
            metrics,
            feedback: None,
        }
    }

    /// Add feedback to assessment
    #[must_use]
    pub fn with_feedback(mut self, feedback: impl Into<String>) -> Self {
        self.feedback = Some(feedback.into());
        self
    }

    /// Check if assessment meets a quality threshold
    #[must_use]
    pub const fn meets_threshold(&self, threshold: u8) -> bool {
        self.overall_score >= threshold
    }

    /// Get score for a specific metric
    #[must_use]
    pub fn metric_score(&self, metric: &QualityMetric) -> Option<u8> {
        self.metrics.iter()
            .find(|(m, _)| m == metric)
            .map(|(_, s)| *s)
    }
}

/// Quality rubric for a specific task type
#[derive(Clone, Debug)]
pub struct QualityRubric {
    /// Required metrics to assess
    pub required_metrics: Vec<QualityMetric>,
    /// Weights for each metric (must sum to 100)
    pub weights: Vec<u8>,
    /// Minimum passing score
    pub passing_threshold: u8,
}

impl Default for QualityRubric {
    fn default() -> Self {
        Self {
            required_metrics: vec![QualityMetric::Overall],
            weights: vec![100],
            passing_threshold: 70,
        }
    }
}

impl QualityRubric {
    /// Create a rubric for creative tasks
    #[must_use]
    pub fn creative() -> Self {
        Self {
            required_metrics: vec![
                QualityMetric::Creativity,
                QualityMetric::Coherence,
                QualityMetric::Relevance,
            ],
            weights: vec![40, 30, 30],
            passing_threshold: 65,
        }
    }

    /// Create a rubric for accuracy-focused tasks
    #[must_use]
    pub fn accuracy_focused() -> Self {
        Self {
            required_metrics: vec![
                QualityMetric::Accuracy,
                QualityMetric::Completeness,
                QualityMetric::Relevance,
            ],
            weights: vec![50, 30, 20],
            passing_threshold: 75,
        }
    }

    /// Calculate weighted score from an assessment
    #[must_use]
    pub fn calculate_weighted_score(&self, assessment: &QualityAssessment) -> u8 {
        if self.required_metrics.len() != self.weights.len() {
            return assessment.overall_score;
        }

        let mut weighted_sum: u32 = 0;
        let mut total_weight: u32 = 0;

        for (metric, weight) in self.required_metrics.iter().zip(self.weights.iter()) {
            if let Some(score) = assessment.metric_score(metric) {
                weighted_sum += u32::from(score) * u32::from(*weight);
                total_weight += u32::from(*weight);
            }
        }

        if total_weight == 0 {
            assessment.overall_score
        } else {
            (weighted_sum / total_weight) as u8
        }
    }

    /// Check if assessment passes this rubric
    #[must_use]
    pub fn passes(&self, assessment: &QualityAssessment) -> bool {
        let score = self.calculate_weighted_score(assessment);
        score >= self.passing_threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_assessment() {
        let assessment = QualityAssessment::simple(85);
        assert!(assessment.meets_threshold(80));
        assert!(!assessment.meets_threshold(90));
    }

    #[test]
    fn test_detailed_assessment() {
        let assessment = QualityAssessment::detailed(vec![
            (QualityMetric::Creativity, 90),
            (QualityMetric::Coherence, 80),
            (QualityMetric::Relevance, 70),
        ]);

        assert_eq!(assessment.overall_score, 80); // Average
        assert_eq!(assessment.metric_score(&QualityMetric::Creativity), Some(90));
    }

    #[test]
    fn test_rubric_weighted_score() {
        let rubric = QualityRubric::creative();

        let assessment = QualityAssessment::detailed(vec![
            (QualityMetric::Creativity, 100),   // 40% weight
            (QualityMetric::Coherence, 80),     // 30% weight
            (QualityMetric::Relevance, 60),     // 30% weight
        ]);

        // (100 * 40 + 80 * 30 + 60 * 30) / 100 = (4000 + 2400 + 1800) / 100 = 82
        let weighted = rubric.calculate_weighted_score(&assessment);
        assert_eq!(weighted, 82);
    }

    #[test]
    fn test_rubric_passes() {
        let rubric = QualityRubric {
            passing_threshold: 75,
            ..QualityRubric::default()
        };

        let passing = QualityAssessment::simple(80);
        let failing = QualityAssessment::simple(70);

        assert!(rubric.passes(&passing));
        assert!(!rubric.passes(&failing));
    }
}
