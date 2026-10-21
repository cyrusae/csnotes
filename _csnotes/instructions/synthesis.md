# csnotes — synthesis philosophy

This file tells you *how* to think about turning raw lecture notes into
synthetic notes.  Read this alongside `claude.md` (which covers the technical
output contract).

---

## Voice and style

**Bullets, not prose.**  The default unit of a note is a punchy bullet point,
grouped under `###` subtopic headings — not flowing paragraphs.  Sentence
fragments are fine and encouraged; write like handwritten class notes, not
like an article.  Prose is reserved for the rare concept that genuinely needs
a connected explanation (a subtle mental model, a "the trap version of this
idea is..." walkthrough) — and even then, keep it to 2–3 sentences before
returning to bullets.

Mechanics:
- **One idea per bullet.**  Elaboration goes in a nested sub-bullet, not a
  longer sentence.
- **Bold the load-bearing words**: pivotal terms on first use, the actual
  gotcha ("**always executes at least once**", "**wraps, doesn't crash**"),
  and rules worth memorizing.  If a reader skims only the bold text, they
  should hit every landmine.
- Use `→` for cause/effect and "therefore" inside a bullet
  ("condition checked first → body can run **zero times**").
- Code blocks carry the examples; the bullets around them point at what the
  code shows.

**Register: informal/colloquial.**  Debrief-a-friend voice — "the one that
gets everybody", "duct-tape fix", "don't cross the streams".  Keep jokes,
snark, and asides from the raw notes; they're mnemonic, not noise.
Precision still matters: conversational doesn't mean vague.  Get the
technical content exactly right; just say it in fragments with the key term
bolded.

**Exception — the embed block.**  The intro paragraph above each `^block-id`
is transcluded into the index, so it stays a single compact paragraph
(2–3 sentences max, fragments fine, key terms bolded) rather than a list.

---

## What gets a note

**Stable, reusable knowledge gets a note.**  Ask: "would this concept come up
again in a different context, or is it a one-time artifact of this lecture?"
If yes → note.  If no → leave it in the raw notes.

**Worked examples** do not get their own notes.  They live in the raw notes
and are referenced from the concept note that they illustrate:
> (worked example in CPSC5001 09-03 lecture)

**Procedural steps** (e.g., insertion algorithm for a data structure) get a
note if the procedure is the point — if understanding it is the goal, not just
executing it.

**Definitions** get folded into the concept note they define, not their own
separate note, unless the definition itself is subtle or contested enough to
be worth isolating.

---

## Granularity

Default to **coarser** notes.  One concept = one atomic note, with subheadings
inside it if the concept has natural parts.

**Split trigger:** create a separate atomic note when:
- A sub-concept appears independently across multiple sessions (it's earning
  its own identity), or
- You want to wikilink to that sub-concept specifically from an unrelated topic
  (it needs to be a standalone target).

Until one of those is true, keep it inside the parent concept note with a
subheading.  You can always split later; merging is harder.

---

## Connections and wikilinks

**Actively make connections.**  If the lecture introduces something that
relates to a concept from a previous session or the course textbook, weave
that connection into the note body and add a wikilink.

Don't just record what the new session said in isolation.  The point of
synthetic notes is accumulated understanding, not a per-session transcript.

If you notice that two things the student wrote in different sessions are the
same concept under different names, say so in the note and link them.

---

## Handling uncertainty

**Source ambiguity** — ambiguous handwriting, shorthand you can't resolve, two
terms that might be the same thing — **make your best guess, write the note,
and flag it**.  Use a `review_flag` with kind `uncertain_content` or
`ambiguous_term` and explain what you weren't sure about and what you decided.
The student will see the flag after the session and can correct it then.

**Comprehension gaps** — if the student is fuzzy on a concept rather than the
source being ambiguous, don't write a hedged note and flag it.  Explore it in
the debrief first: ask them to explain it back, work through an example
together, ask follow-up questions.  Write the note from what emerges in
conversation.  A note that reflects understanding after the conversation is
worth more than one that accurately records initial confusion.  Flags are a
fallback for things that genuinely couldn't be resolved, not a first response
to fuzziness.

Don't leave placeholders or refuse to write the note.  A flagged imperfect
note is more useful than a blank.

---

## Index notes

The index note for a topic (`_synthetic/<topic>/<topic>.md`) carries:

1. **An orientation paragraph** (2–4 sentences): what this topic is, why it
   matters in the course, and the shape of the material — what the atomics
   cover and how they fit together.  Write this in the same conversational
   voice as the atomics.  It should orient someone who hasn't looked at this
   topic in three weeks.

2. **The embed list** — `![[atomic-slug#^block-id]]` lines for each atomic.
   The CLI auto-inserts embed lines for new notes declared with `embed_in` in
   `create_note` — you don't need to write them manually.  For existing embeds,
   maintain the ordering.

The orientation paragraph should be updated (via `update_note`) when the scope
of the topic changes significantly — e.g., when a topic that started as "basic
sorting" expands to cover advanced variants.  Don't update it after every
session just because new atomics were added.

---

## Depth follows the student, not the source

The right depth for a note is the depth of the student's actual understanding —
not a ceiling set by what the lecture explicitly covered, and not a floor set
by what the source happens to say.

At the start of the debrief, ask what the student was working from and what
landed.  That answer — not the file structure — tells you which source's
framing to lean on and how far to go.

- If the student worked from lecture notes, synthesise from the lecture's
  framing: what did the instructor emphasise, what angle did they take?
- If the student read ahead or finished a textbook chapter, that source can
  be primary for this session.  Don't hold back depth just because the lecture
  hasn't caught up — write to what the student actually absorbed.
- If the student brings in personal notes, highlights, or an AI conversation
  that developed a concept further, draw on it.  Cross-source synthesis is
  encouraged.

When a concept spans sources, add a wikilink and a brief note
("connection not yet formalised in lecture") rather than silently merging
framings that don't agree yet.

---

## What success looks like after a session

After processing a session, the vault should have:

- One index note per topic engaged with, with an orientation paragraph that
  would orient you if you read it cold in three weeks.
- One atomic note per stable concept the student actually understands — written
  at the granularity that felt natural from the conversation (err coarser; you
  can split later).
- Depth that matches what emerged in the debrief, not a safe minimum anchored
  to the source's coverage.
- Wikilinks to any concepts that connect to prior knowledge, even if those
  target notes don't exist yet — broken wikilinks get flagged, which is fine;
  it surfaces what needs to be created in a future session.
- Any worked examples referenced by name in the concept notes, not turned into
  their own notes.
- A short list of `review_flags` for anything that couldn't be resolved in
  conversation.

What it should *not* look like: a reformatted version of the lecture outline,
a note-per-slide, something that mirrors textbook chapter structure, or notes
that are deliberately vague because the student seemed uncertain at the start
of the conversation.
The question to ask is "would this help me in three weeks?" not "does this
accurately transcribe what the source said?"
