use volt::skill_scorer::{SkillActivation, SkillManifest, SkillScorer};

#[test]
fn test_keyword_match_full_word() {
    let activation = SkillActivation {
        keywords: vec!["deploy".into(), "production".into()],
        ..Default::default()
    };
    let skill = make_skill("deploy", activation);
    // "deploy to production" contains both keywords as full words
    assert!(SkillScorer::score("deploy to production", &skill) >= 20);
}

#[test]
fn test_keyword_match_substring() {
    let activation = SkillActivation {
        keywords: vec!["deploy".into()],
        ..Default::default()
    };
    let skill = make_skill("deploy", activation);
    // "deployment" contains "deploy" as substring
    assert_eq!(SkillScorer::score("deployment process", &skill), 5);
}

#[test]
fn test_no_match() {
    let activation = SkillActivation {
        keywords: vec!["k8s".into()],
        ..Default::default()
    };
    let skill = make_skill("k8s", activation);
    assert_eq!(SkillScorer::score("hello world", &skill), 0);
}

#[test]
fn test_exclusion_veto() {
    let activation = SkillActivation {
        keywords: vec!["deploy".into()],
        exclude_keywords: vec!["rollback".into()],
        ..Default::default()
    };
    let skill = make_skill("deploy", activation);
    assert_eq!(SkillScorer::score("deploy to prod", &skill), 10);
    assert_eq!(SkillScorer::score("deploy rollback", &skill), 0);
}

fn make_skill(name: &str, activation: SkillActivation) -> SkillManifest {
    SkillManifest {
        name: name.into(),
        version: "1.0.0".into(),
        description: "test skill".into(),
        content: "".into(),
        mcp_servers: vec![],
        source_path: None,
        activation,
    }
}
