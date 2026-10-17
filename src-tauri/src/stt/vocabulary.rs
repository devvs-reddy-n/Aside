//! Focused vocabulary for technical conversations. Extract terms, not every
//! sentence-initial capital, and keep names, short acronyms and phrases intact.

pub const CAP: usize = 64;
const TECH: &[&str] = &[
    "AWS", "GCP", "Azure", "SQL", "NoSQL", "MySQL", "PostgreSQL", "Postgres",
    "Redis", "Kafka", "Kubernetes", "k8s", "Docker", "Terraform", "Helm",
    "Python", "Rust", "Java", "JavaScript", "TypeScript", "C++", "C#", ".NET",
    "Golang", "React", "Next.js", "Node.js", "GraphQL", "gRPC", "REST API",
    "CI/CD", "GitHub Actions", "GitLab", "Jenkins", "Linux", "Spark", "Redshift",
    "Snowflake", "Databricks", "dbt", "Airflow", "ETL", "DynamoDB", "S3", "EC2",
    "Lambda", "IAM", "OAuth", "JWT", "TCP", "UDP", "HTTP", "HTTPS", "DNS",
    "RAG", "MCP", "LLM", "LLMs", "Kiro", "LangChain", "LangGraph", "LlamaIndex",
    "PyTorch", "TensorFlow", "ONNX", "OpenAI", "Anthropic", "Claude", "GPT",
    "machine learning", "deep learning", "vector database", "retrieval augmented generation",
    "model context protocol", "rate limiter", "token bucket", "eventual consistency",
    "idempotency", "microservices", "observability", "OpenTelemetry", "Prometheus", "Grafana",
];

fn contains_term(text: &str, term: &str) -> bool {
    let text = text.to_lowercase();
    let term = term.to_lowercase();
    text.match_indices(&term).any(|(start, _)| {
        let end = start + term.len();
        !text[..start].chars().next_back().is_some_and(char::is_alphanumeric)
            && !text[end..].chars().next().is_some_and(char::is_alphanumeric)
    })
}

pub fn merge(target: &mut Vec<String>, values: impl IntoIterator<Item = String>) {
    for value in values {
        let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
        if value.is_empty() || value.len() > 160 || value.split_whitespace().count() > 6 { continue; }
        if !target.iter().any(|t| t.eq_ignore_ascii_case(&value)) {
            target.push(value);
        }
        if target.len() >= CAP { break; }
    }
    target.truncate(CAP);
}

pub fn extract(company: Option<&str>, text: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    merge(&mut out, company.into_iter().map(str::to_owned));
    let text = text.unwrap_or_default();
    merge(&mut out, TECH.iter().filter(|term| contains_term(text, term)).map(|s| (*s).to_owned()));
    let stop = ["about", "across", "after", "also", "and", "are", "based", "build", "company",
        "design", "develop", "development", "engineer", "engineering", "experience", "have",
        "including", "knowledge", "must", "our", "preferred", "required", "requirements",
        "responsibilities", "role", "senior", "skills", "strong", "team", "technical", "the",
        "this", "using", "with", "work", "years", "you", "your"];
    let words = text.split(|c: char| !c.is_alphanumeric() && !matches!(c, '+' | '#' | '.' | '/' | '-'));
    let candidates = words.filter_map(|word| {
        let word = word.trim_matches(|c: char| matches!(c, '.' | '/' | '-'));
        if word.len() < 3 || word.len() > 40 || stop.contains(&word.to_lowercase().as_str()) { return None; }
        let acronym = word.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase())
            && word.chars().any(|c| c.is_alphabetic());
        let name = word.chars().next().is_some_and(char::is_uppercase)
            && word.chars().any(char::is_lowercase);
        (acronym || name).then(|| word.to_owned())
    });
    merge(&mut out, candidates);
    out
}

/// Include the normal spelling too: sounds_like replaces, rather than adds
/// to, the provider's automatically generated pronunciation.
pub fn entry(term: &str, english: bool) -> serde_json::Value {
    let mut value = serde_json::json!({"content": term});
    if !english { return value; }
    let variants: &[&str] = match term.to_ascii_lowercase().as_str() {
        "kiro" => &["key row"],
        "kubernetes" | "k8s" => &["koo ber net ees", "kubernetes", "k eights"],
        "grpc" => &["G.R.P.C.", "gee are pee see"],
        "sql" => &["S.Q.L.", "sequel"],
        "postgresql" => &["post gres Q.L.", "post gres sequel"],
        "postgres" => &["post gres"],
        "rag" => &["rag", "R.A.G."],
        "mcp" => &["M.C.P."],
        "llm" => &["L.L.M."],
        "llms" => &["L.L.M.s"],
        "ci/cd" => &["C.I.C.D.", "continuous integration continuous delivery"],
        "c++" => &["C plus plus"],
        "c#" => &["C sharp"],
        "onnx" => &["onyx", "O.N.N.X."],
        _ => &[],
    };
    if !variants.is_empty() {
        let mut pronunciations = vec![term];
        pronunciations.extend(variants.iter().copied());
        value["sounds_like"] = serde_json::json!(pronunciations);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_lowercase_and_punctuated_terms_and_phrases_survive() {
        let out = extract(Some("Acme"), Some("Experience with Kiro, MCP, RAG, LLM, C++, C#, dbt, gRPC and model context protocol. Strong technical skills required."));
        for term in ["Acme", "Kiro", "MCP", "RAG", "LLM", "C++", "C#", "dbt", "gRPC", "model context protocol"] {
            assert!(out.iter().any(|t| t == term), "missing {term}: {out:?}");
        }
        for generic in ["Experience", "Strong", "technical"] { assert!(!out.iter().any(|t| t == generic)); }
    }

    #[test]
    fn matching_does_not_invent_technologies_from_substrings() {
        let out = extract(None, Some("s3bucket and PostgreSQL, not Rusty or TypeScripted"));
        assert!(!out.iter().any(|t| ["S3", "Rust", "TypeScript"].contains(&t.as_str())));
        assert!(out.iter().any(|t| t == "PostgreSQL"));
    }

    #[test]
    fn canonical_spelling_and_pronunciations_are_separate_and_bounded() {
        let value = entry("MCP", true);
        assert_eq!(value["content"], "MCP");
        assert!(value["sounds_like"].as_array().unwrap().iter().any(|v| v == "M.C.P."));
        assert!(entry("MCP", false).get("sounds_like").is_none());
        let mut out = vec!["MCP".into()];
        merge(&mut out, ["mcp".into(), "one two three four five six seven".into()]);
        assert_eq!(out, vec!["MCP"]);
    }
}
