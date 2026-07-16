use std::collections::HashMap;

pub struct Bm25Index {
    postings: HashMap<String, Vec<(usize, f32)>>,
    idf: HashMap<String, f64>,
    doc_lengths: Vec<f32>,
    avg_dl: f32,
    num_docs: usize,
    k1: f32,
    b: f32,
}

impl Bm25Index {
    pub fn new(documents: &[Vec<String>]) -> Self {
        let num_docs = documents.len();
        let mut postings: HashMap<String, Vec<(usize, f32)>> = HashMap::new();
        let mut doc_lengths = Vec::with_capacity(num_docs);
        let mut df: HashMap<String, usize> = HashMap::new();

        for (doc_id, tokens) in documents.iter().enumerate() {
            doc_lengths.push(tokens.len() as f32);

            let mut tf: HashMap<&str, f32> = HashMap::new();
            for token in tokens {
                *tf.entry(token.as_str()).or_default() += 1.0;
            }

            for (term, freq) in tf {
                postings
                    .entry(term.to_string())
                    .or_default()
                    .push((doc_id, freq));
                *df.entry(term.to_string()).or_default() += 1;
            }
        }

        let avg_dl = if num_docs > 0 {
            doc_lengths.iter().sum::<f32>() / num_docs as f32
        } else {
            0.0
        };

        let idf: HashMap<String, f64> = df
            .iter()
            .map(|(term, &doc_freq)| {
                let n = num_docs as f64;
                let df = doc_freq as f64;
                let idf_val = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                (term.clone(), idf_val)
            })
            .collect();

        Self {
            postings,
            idf,
            doc_lengths,
            avg_dl,
            num_docs,
            k1: 1.5,
            b: 0.75,
        }
    }

    pub fn get_scores(&self, query_tokens: &[String], weight_mask: Option<&[bool]>) -> Vec<f32> {
        let mut scores = vec![0.0f32; self.num_docs];

        for token in query_tokens {
            let idf = match self.idf.get(token.as_str()) {
                Some(&v) => v as f32,
                None => continue,
            };

            if let Some(posting_list) = self.postings.get(token.as_str()) {
                for &(doc_id, tf) in posting_list {
                    if let Some(mask) = weight_mask {
                        if doc_id >= mask.len() || !mask[doc_id] {
                            continue;
                        }
                    }

                    let dl = self.doc_lengths[doc_id];
                    let tf_component = (tf * (self.k1 + 1.0))
                        / (tf + self.k1 * (1.0 - self.b + self.b * dl / self.avg_dl));
                    scores[doc_id] += idf * tf_component;
                }
            }
        }

        scores
    }

    pub fn num_docs(&self) -> usize {
        self.num_docs
    }
}
