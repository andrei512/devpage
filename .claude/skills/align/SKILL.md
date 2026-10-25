---
name: align
description: Helps humans and agents align through focused conversation. Use when the user asks to align, discuss, explore, or clarify, or when a complex request has multiple plausible interpretations.
---

# Align

Help the human and agent develop shared understanding. Apply this conversation
protocol alongside the active domain skill; that skill still owns the subject,
decisions and next actions.

## Focus

- Follow the abstraction level the human is discussing.
- Work on one topic at a time, starting high-level and going deeper only where
  useful.
- Keep each response within one terminal screen, roughly 200 words by default.
- Ask at most one focused question per response.
- When the human is steering, answer and let them continue. When the agent needs
  to steer, ask the highest-value unresolved question and recommend an answer
  when useful.
- Investigate before asking. Read the repository and consult reliable sources
  for factual answers.
- Ask the human only for intent, preferences, inaccessible information, or
  genuine decisions with multiple valid answers. If research cannot resolve
  something, state what was checked and ask one precise question.
- Surface assumptions that could materially change the shared view.

Do not dump a complete plan, ask a batch of questions, or rush toward an
artifact.

## Align recursively

Occasionally summarize the current shared view at the active zoom level. A
summary is a test for misunderstanding, not a declaration of agreement.

When the human corrects part of the summary:

1. Select that one disagreement.
2. Discuss it at the next useful level of detail.
3. Compress the resolved detail back into the higher-level view.
4. Recheck the whole view only when the correction could affect it.

## Readiness

Stay curious rather than trying to complete alignment quickly. Never claim that
the human is aligned.

When nothing material remains unclear, say: `I have no more questions.`

The human decides when alignment is sufficient. This skill never writes files,
changes state or authorizes the calling skill's next action.
