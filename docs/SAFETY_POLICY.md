# Safety policy

Quisum is a self-reflection product for adults. It is not medical care, psychotherapy, diagnosis, or an emergency service.

## Prohibited behavior

The product and official methods must not:

- diagnose depression, anxiety disorders, ADHD, PTSD, attachment disorders, personality disorders, or other conditions from conversation;
- present a model inference as clinical fact, a hidden motive, or an unconscious truth;
- use leading questions to invent or recover childhood memories;
- generalize a transient state or one event into a stable identity;
- encourage secrecy from, withdrawal from, or replacement of real relationships;
- imply that the assistant is uniquely understanding, possessive, hurt by departure, or dependent on continued use;
- analyze a third party as if Quisum had consent to build that person's profile;
- retain acute-crisis language as durable identity evidence;
- make high-impact decisions about employment, education, housing, credit, insurance, or legal status.

## Allowed framing

The assistant may identify an observable recurrence, state that an explanation is tentative, offer alternatives, ask where it does not apply, and suggest professional assessment when symptoms materially affect daily life. Mentioning a concept is not a diagnosis.

## Crisis route

The deterministic safety pre-check and post-check surround all model interaction. When there is a credible indication of imminent self-harm, harm to another person, or acute inability to remain safe:

1. Stop personality and trauma analysis.
2. Ask directly and plainly about immediate danger where an interactive response is possible.
3. Encourage contacting local emergency services and a trusted person who can help without putting themselves in danger.
4. Display source-linked region-specific resources from a versioned local resource pack while its source check is current.
5. Avoid deep exploration, speculative causes, or promises of confidentiality.
6. Mark the content crisis-related and exclude it from durable inference.

The product does not claim that automated detection is complete. UI copy tells users it is not an emergency service.

The V1 resource pack covers only explicitly listed regions and otherwise uses a no-number fallback. Desktop selection is inferred from the app locale, not physical location, so the chosen region can be wrong. Availability is not checked at display time. Once the pack's source-check due date passes, Core suppresses region-specific numbers and uses the generic emergency-services fallback until maintainers verify and version the sources again. A maintainer source check is not clinical or professional review.

## Memory safety

Pending, rejected, crisis-related, expired, irrelevant highly sensitive, and `local_only` content is excluded from ordinary model retrieval. Provider requests pass a context packet through an outbound policy check. Stored text is quoted as untrusted data and never concatenated into system instructions.

Safety routing applies to the complete user-authored durable payload, not only its headline field. Event topics, claim contexts and alternatives, review edits and notes, session titles, context purposes and queries, experiment expectations, results, and learning notes are rechecked at the Vault boundary. An adapter that omits a field-level precheck therefore cannot persist acute-risk text through a secondary field.

## Evaluation gates

Changes to prompts, retrieval, claim extraction, crisis logic, or provider handling require cases covering overdiagnosis, overconfidence, false memory, contradiction, dependency, crisis behavior, and privacy leakage. A release fails when a critical safety invariant is violated, even if conversational quality improves.

`quisum-eval` executes all repository cases. Its offline mode verifies deterministic policy and the assertion harness only. Provider compatibility requires a complete, timestamped result set for one exact adapter, model, capability class, and prompt bundle; results never transfer to another model or version.

## Professional review

Official method packages must identify their evidence basis and review status. “Professionally reviewed” cannot be claimed until named qualified reviewers have reviewed the published version. V1.0 release metadata must accurately report that status.
