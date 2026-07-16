use anyhow::{Context, Result};
use model2vec_rs::model::StaticModel;
use ndarray::{Array1, Array2, Axis};

const DEFAULT_MODEL_NAME: &str = "minishlab/potion-code-16M";

pub struct StaticEncoder {
    model: StaticModel,
    dim: usize,
}

impl StaticEncoder {
    pub fn load(model_name: Option<&str>) -> Result<Self> {
        // Priority: explicit model_name (CLI --model) > SEMBLE_MODEL_PATH env > default
        let path_or_repo = match model_name {
            Some(m) => m.to_string(),
            None => std::env::var("SEMBLE_MODEL_PATH")
                .unwrap_or_else(|_| DEFAULT_MODEL_NAME.to_string()),
        };
        let model = StaticModel::from_pretrained(&path_or_repo, None, None, None)
            .with_context(|| format!("Failed to load model from {path_or_repo}"))?;
        let dim = model.encode_single("a").len();
        Ok(Self { model, dim })
    }

    pub fn embedding_dim(&self) -> usize {
        self.dim
    }

    pub fn encode_single(&self, text: &str) -> Result<Array1<f32>> {
        let v = self.model.encode_single(text);
        Ok(Array1::from_vec(v))
    }

    pub fn encode_batch(&self, texts: &[String]) -> Result<Array2<f32>> {
        if texts.is_empty() {
            return Ok(Array2::zeros((0, self.dim)));
        }
        let vecs = self.model.encode(texts);
        let n = vecs.len();
        let flat: Vec<f32> = vecs.into_iter().flatten().collect();
        Array2::from_shape_vec((n, self.dim), flat).context("Failed to reshape embeddings")
    }
}

pub struct SemanticIndex {
    embeddings: Array2<f32>,
}

impl SemanticIndex {
    pub fn new(mut embeddings: Array2<f32>) -> Self {
        for mut row in embeddings.axis_iter_mut(Axis(0)) {
            let norm: f32 = row.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 1e-12 {
                row.mapv_inplace(|x| x / norm);
            }
        }
        Self { embeddings }
    }

    pub fn query(
        &self,
        query_embedding: &Array1<f32>,
        k: usize,
        selector: Option<&[usize]>,
    ) -> Vec<(usize, f32)> {
        let norm: f32 = query_embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        let query_norm = if norm > 1e-12 {
            query_embedding.mapv(|x| x / norm)
        } else {
            query_embedding.clone()
        };

        if let Some(selector) = selector {
            let mut dists: Vec<(usize, f32)> = selector
                .iter()
                .filter(|&&idx| idx < self.embeddings.nrows())
                .map(|&idx| {
                    let sim: f32 = self.embeddings.row(idx).dot(&query_norm);
                    (idx, 1.0 - sim)
                })
                .collect();
            dists.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            dists.truncate(k);
            dists
        } else {
            let similarities = self.embeddings.dot(&query_norm);
            let mut dists: Vec<(usize, f32)> = similarities
                .iter()
                .enumerate()
                .map(|(idx, &sim)| (idx, 1.0 - sim))
                .collect();
            if k < dists.len() {
                dists.select_nth_unstable_by(k, |a, b| {
                    a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
                });
                dists.truncate(k);
            }
            dists.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            dists
        }
    }
}
