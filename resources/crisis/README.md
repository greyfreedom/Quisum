# Crisis resource pack

Quisum embeds [v1.0.0.json](v1.0.0.json) into Core. It is a bounded immediate-support aid, not a directory service, medical advice, professional review, or a guarantee that a number is reachable.

The current pack has four explicit regions plus a no-number fallback:

- United States: [SAMHSA crisis guidance](https://www.samhsa.gov/find-support/in-crisis) and [SAMHSA 988 key messages](https://www.samhsa.gov/mental-health/988/key-messages).
- China mainland: [Beijing government/National Health Commission 120 guidance](https://www.beijing.gov.cn/fuwu/bmfw/sy/jrts/202607/t20260713_4759730.html) and [China government emergency numbers](https://www.gov.cn/ztzl/yjzn/content_562406.htm).
- United Kingdom: [NHS urgent mental-health support](https://www.nhs.uk/every-mind-matters/urgent-support/) and [NHS 999 guidance](https://www.nhs.uk/nhs-services/urgent-and-emergency-care-services/when-to-call-999/).
- Australia: [Lifeline Crisis Support](https://www.lifeline.org.au/get-help/national-services/lifeline-crisis-support).

The listed sources were checked by the project maintainer on 2026-08-12 and must be checked again by 2026-11-12. This records source verification only; it does not claim a clinical, emergency-services, or professional safety review.

## Runtime behavior

The desktop passes its locale as a hint; a locale is not a physical location. Unsupported or malformed regions use the fallback. After `source_check_due`, Core suppresses every region-specific number and source address and uses the generic local-emergency-services message. No lookup or telemetry request occurs.

## Update procedure

1. Verify each number, eligibility condition, hours, language limitation, and URL against the named authoritative source.
2. Remove a region if its current details cannot be verified; never preserve a number merely for coverage.
3. Update `source_checked_at` and `source_check_due`. Increment `pack_version` for any material wording, number, region, or source change.
4. Validate against [the resource-pack schema](../../schemas/crisis-resource-pack.schema.json), run Core tests, and run all executable evals.
5. Record the change in the changelog. Keep professional-review status separate from source checking.
