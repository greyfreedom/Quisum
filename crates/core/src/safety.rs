use serde::{Deserialize, Serialize};

use crate::DomainError;

/// Deterministic safety route selected before or after model generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyRoute {
    /// Continue the selected reflective workflow.
    Continue,
    /// Pause analysis and ask a bounded safety or intent question.
    Pause,
    /// Stop identity analysis and show immediate-support guidance.
    Crisis,
    /// Refuse covert analysis of a third party.
    RefuseThirdPartyProfiling,
}

/// Machine-readable reason code that does not repeat private input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyReason {
    /// Signals of imminent self-harm.
    ImminentSelfHarm,
    /// Signals of imminent harm to another person.
    ImminentHarmToOther,
    /// Request to create a profile of another person.
    ThirdPartyProfiling,
    /// Generated text declared a diagnosis.
    DiagnosticOverclaim,
    /// Generated text claimed hidden or unconscious truth.
    HiddenTruthOverclaim,
    /// Generated text used leading false-memory language.
    FalseMemoryInduction,
    /// Generated text encouraged emotional dependence.
    DependencyLanguage,
}

/// Result of deterministic safety screening.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafetyDecision {
    /// Selected route.
    pub route: SafetyRoute,
    /// Content-free reason codes.
    pub reasons: Vec<SafetyReason>,
    /// Whether ordinary durable candidate persistence is allowed.
    pub allow_persistence: bool,
}

impl SafetyDecision {
    /// Normal continuation.
    #[must_use]
    pub const fn continue_normally() -> Self {
        Self {
            route: SafetyRoute::Continue,
            reasons: Vec::new(),
            allow_persistence: true,
        }
    }
}

/// A conservative local safety layer independent of provider prompts.
#[derive(Debug, Default, Clone, Copy)]
pub struct SafetyEngine;

impl SafetyEngine {
    /// Screen user input before retrieval or model analysis.
    #[must_use]
    pub fn screen_user_input(self, input: &str) -> SafetyDecision {
        let normalized = normalize(input);
        if indicates_imminent_self_harm(&normalized) {
            return SafetyDecision {
                route: SafetyRoute::Crisis,
                reasons: vec![SafetyReason::ImminentSelfHarm],
                allow_persistence: false,
            };
        }
        if indicates_imminent_harm_to_other(&normalized) {
            return SafetyDecision {
                route: SafetyRoute::Crisis,
                reasons: vec![SafetyReason::ImminentHarmToOther],
                allow_persistence: false,
            };
        }
        if asks_for_third_party_profile(&normalized) {
            return SafetyDecision {
                route: SafetyRoute::RefuseThirdPartyProfiling,
                reasons: vec![SafetyReason::ThirdPartyProfiling],
                allow_persistence: false,
            };
        }
        SafetyDecision::continue_normally()
    }

    /// Inspect generated text for non-negotiable overclaim and dependency violations.
    #[must_use]
    pub fn review_generated_text(self, output: &str) -> SafetyDecision {
        let normalized = normalize(output);
        let mut reasons = Vec::new();
        if contains_any(
            &normalized,
            &[
                "you have adhd",
                "you have depression",
                "you have an anxiety disorder",
                "you are a narcissist",
                "you are avoidantly attached",
                "this proves you have",
                "你有抑郁症",
                "你有焦虑症",
                "你有 adhd",
                "你患有抑郁症",
                "你患有焦虑症",
                "你是自恋型人格",
                "你是回避型依恋",
                "这证明你有",
            ],
        ) {
            reasons.push(SafetyReason::DiagnosticOverclaim);
        }
        if contains_any(
            &normalized,
            &[
                "your unconscious really wants",
                "your subconscious actually",
                "the hidden truth about you is",
                "你的潜意识其实",
                "你内心真正想要的其实",
                "关于你的隐藏真相是",
            ],
        ) {
            reasons.push(SafetyReason::HiddenTruthOverclaim);
        }
        if contains_any(
            &normalized,
            &[
                "when your parents neglected you",
                "the trauma you repressed",
                "recover the hidden memory",
                "当年你父母忽视你的时候",
                "你压抑的童年记忆",
                "恢复那段被压抑的记忆",
            ],
        ) {
            reasons.push(SafetyReason::FalseMemoryInduction);
        }
        if contains_any(
            &normalized,
            &[
                "only i understand you",
                "you only need me",
                "don't leave me",
                "i will be hurt if you leave",
                "只有我理解你",
                "只有我懂你",
                "你只需要我",
                "不要离开我",
                "你离不开我",
                "每天都要回来找我",
            ],
        ) {
            reasons.push(SafetyReason::DependencyLanguage);
        }

        if reasons.is_empty() {
            SafetyDecision::continue_normally()
        } else {
            SafetyDecision {
                route: SafetyRoute::Pause,
                reasons,
                allow_persistence: false,
            }
        }
    }

    /// Require a normal safety route before persisting model-derived candidates.
    pub fn require_persistence_allowed(self, decision: &SafetyDecision) -> Result<(), DomainError> {
        if decision.route == SafetyRoute::Continue && decision.allow_persistence {
            Ok(())
        } else {
            Err(DomainError::SafetyBlocked)
        }
    }
}

fn normalize(input: &str) -> String {
    input
        .chars()
        .flat_map(char::to_lowercase)
        .map(|character| {
            if character.is_whitespace() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn contains_any(input: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| input.contains(needle))
}

fn indicates_imminent_self_harm(input: &str) -> bool {
    if contains_any(
        input,
        &[
            "i am going to kill myself",
            "i'm going to kill myself",
            "i will kill myself tonight",
            "i plan to end my life tonight",
            "我要自杀",
            "我今晚要自杀",
            "我现在就去死",
        ],
    ) {
        return true;
    }
    let subject = contains_any(
        input,
        &[
            "suicide",
            "kill myself",
            "end my life",
            "hurt myself",
            "自杀",
            "结束生命",
            "结束自己的生命",
            "伤害自己",
        ],
    );
    let plan = contains_any(
        input,
        &["plan", "planned", "going to", "打算", "计划", "准备"],
    );
    let immediate = contains_any(
        input,
        &[
            "tonight",
            "today",
            "right now",
            "now",
            "今晚",
            "今天",
            "现在",
        ],
    );
    let means = contains_any(
        input,
        &[
            "have the means",
            "pills",
            "weapon",
            "gun",
            "knife",
            "药",
            "武器",
            "枪",
            "刀",
        ],
    );
    subject
        && [plan, immediate, means]
            .into_iter()
            .filter(|value| *value)
            .count()
            >= 2
}

fn indicates_imminent_harm_to_other(input: &str) -> bool {
    if contains_any(
        input,
        &[
            "i am going to kill him",
            "i am going to kill her",
            "i am going to kill them",
            "我要杀了他",
            "我要杀了她",
            "我要杀了他们",
        ],
    ) {
        return true;
    }
    let harm = contains_any(
        input,
        &[
            "hurt my coworker",
            "kill my coworker",
            "hurt someone",
            "kill someone",
            "伤害同事",
            "杀人",
            "伤害别人",
            "杀了他",
            "杀了她",
            "杀了他们",
            "伤害他",
            "伤害她",
            "伤害他们",
        ],
    );
    let immediate = contains_any(
        input,
        &[
            "tonight",
            "today",
            "right now",
            "now",
            "今晚",
            "今天",
            "现在",
        ],
    );
    let means = contains_any(input, &["weapon", "gun", "knife", "武器", "枪", "刀"]);
    harm && (immediate || means)
}

fn asks_for_third_party_profile(input: &str) -> bool {
    contains_any(
        input,
        &[
            "analyze my partner's personality",
            "diagnose my partner",
            "build a profile of my coworker",
            "secretly analyze",
            "分析我伴侣的人格",
            "诊断我的伴侣",
            "给我同事建立人格画像",
            "给他做人格画像",
            "给她做人格画像",
            "秘密分析",
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imminent_self_harm_uses_crisis_route_and_blocks_persistence() {
        let decision = SafetyEngine
            .screen_user_input("I have the means beside me and plan to hurt myself tonight.");
        assert_eq!(decision.route, SafetyRoute::Crisis);
        assert!(!decision.allow_persistence);
        assert_eq!(decision.reasons, vec![SafetyReason::ImminentSelfHarm]);
    }

    #[test]
    fn imminent_harm_to_other_uses_crisis_route() {
        let decision = SafetyEngine
            .screen_user_input("I may seriously hurt my coworker today and I have a weapon.");
        assert_eq!(decision.route, SafetyRoute::Crisis);
        assert!(!decision.allow_persistence);
    }

    #[test]
    fn chinese_imminent_harm_phrases_use_the_same_crisis_route() {
        for input in [
            "我计划今天杀了他，刀就在旁边。",
            "我现在准备结束自己的生命。",
        ] {
            let decision = SafetyEngine.screen_user_input(input);
            assert_eq!(decision.route, SafetyRoute::Crisis);
            assert!(!decision.allow_persistence);
        }
    }

    #[test]
    fn ordinary_distress_does_not_become_a_crisis_by_keyword_alone() {
        let decision = SafetyEngine
            .screen_user_input("Last year I had thoughts about suicide, but I am safe now.");
        assert_eq!(decision, SafetyDecision::continue_normally());
    }

    #[test]
    fn dependency_and_diagnosis_output_is_blocked() {
        let decision = SafetyEngine
            .review_generated_text("Only I understand you. This proves you have depression.");
        assert_eq!(decision.route, SafetyRoute::Pause);
        assert!(decision.reasons.contains(&SafetyReason::DependencyLanguage));
        assert!(
            decision
                .reasons
                .contains(&SafetyReason::DiagnosticOverclaim)
        );
        assert!(!decision.allow_persistence);
    }

    #[test]
    fn chinese_dependency_and_diagnosis_output_is_blocked() {
        let decision = SafetyEngine.review_generated_text("只有我懂你，这证明你有焦虑症。");
        assert_eq!(decision.route, SafetyRoute::Pause);
        assert!(decision.reasons.contains(&SafetyReason::DependencyLanguage));
        assert!(
            decision
                .reasons
                .contains(&SafetyReason::DiagnosticOverclaim)
        );
        assert!(!decision.allow_persistence);
    }

    #[test]
    fn third_party_profiling_is_refused() {
        let decision =
            SafetyEngine.screen_user_input("Secretly analyze my partner's personality for me.");
        assert_eq!(decision.route, SafetyRoute::RefuseThirdPartyProfiling);
        assert!(!decision.allow_persistence);
    }
}
