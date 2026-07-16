use crate::ranking::boosting::is_symbol_query;

const ALPHA_SYMBOL: f64 = 0.3;
const ALPHA_NL: f64 = 0.5;

pub fn resolve_alpha(query: &str, alpha: Option<f64>) -> f64 {
    if let Some(a) = alpha {
        return a;
    }
    if is_symbol_query(query) {
        ALPHA_SYMBOL
    } else {
        ALPHA_NL
    }
}
