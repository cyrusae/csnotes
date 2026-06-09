# csnotes — synthesis philosophy

This file tells you *how* to think about turning raw lecture notes into
synthetic notes.  Read this alongside `claude.md` (which covers the technical
output contract).

---

## Voice and style

Write the way you'd debrief a friend who missed class but is smart and cares
about the material.  Conversational, informal, occasionally irreverent.
**Not** textbook prose, not neutral-encyclopedic, not "Polymorphism is defined
as...".  More like "okay so the key thing here is..." or "this is the part
where it actually matters."

If the raw notes contain a joke, a sarcastic aside, or a bit of personality —
keep it.  It's there for a reason: it made the concept stick in the moment.

Precision still matters.  Conversational doesn't mean vague.  Get the
technical content right; just don't write like a Wikipedia article.

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

When you're not sure you understood the raw notes correctly — ambiguous
handwriting, shorthand you can't resolve, two terms that might be the same
thing — **make your best guess, write the note, and flag it**.

Use a `review_flag` with kind `uncertain_content` or `ambiguous_term` and
explain what you weren't sure about and what you decided.  The student will
see the flag after the session is processed and can correct it then.

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
   The CLI manages insertion of new embeds; you write the paragraph and
   maintain ordering.

The orientation paragraph should be updated (via `update_note`) when the scope
of the topic changes significantly — e.g., when a topic that started as "basic
sorting" expands to cover advanced variants.  Don't update it after every
session just because new atomics were added.

---

## On textbook vs. lecture synthesis

<!-- NOTE FOR CYRUS: this section is a placeholder based on what I know so far.
     React to it once you've seen a first session processed. -->

The raw lecture notes are the primary input.  If the lecture covers something
the textbook also covers, synthesise from *the lecture's framing* — what did
the instructor emphasise, what angle did they take?  The textbook framing can
be a source of connections and additional precision, but the note should read
like it came from the course, not from the book.

Cross-chapter synthesis (concepts that span multiple textbook chapters) is
fine and encouraged.  If the lecture doesn't make the connection explicit,
add a wikilink and a brief note like "(also see [[related-concept]] —
connection not yet drawn in lecture)" rather than silently merging them.

---

## What success looks like after a session

After processing a session, the vault should have:

- One index note per topic introduced in the lecture, with an orientation
  paragraph that would orient you if you read it cold in three weeks.
- One atomic note per stable concept introduced, written at the granularity
  that felt natural from the lecture (err coarser; you can split later).
- Wikilinks to any concepts that connect to prior knowledge, even if those
  target notes don't exist yet — broken wikilinks get flagged, which is fine;
  it surfaces what needs to be created in a future session.
- Any worked examples from class referenced by name in the concept notes, not
  turned into their own notes.
- A short list of `review_flags` for anything you weren't sure about.

What it should *not* look like: a reformatted version of the lecture outline,
or a note-per-slide, or anything that mirrors the textbook chapter structure.
The question to ask is "would this help me in three weeks?" not "does this
accurately transcribe what happened in class?"
