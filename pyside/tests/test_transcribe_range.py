from app.models.captions import CaptionSegment, WordSpan
from app.services import transcribe_range as tr

MIN = 60_000


def test_a_short_recording_is_transcribed_whole():
    assert tr.plan(5 * MIN, 0, 5 * MIN) == tr.Plan("whole")


def test_a_long_recording_without_marks_asks_for_a_range():
    plan = tr.plan(47 * MIN, 0, 47 * MIN)
    assert plan.kind == "ask" and plan.duration_ms == 47 * MIN


def test_a_long_recording_with_marks_is_transcribed_between_them():
    assert tr.plan(47 * MIN, 10 * MIN, 20 * MIN) == tr.Plan("range", 10 * MIN, 20 * MIN)


def test_marks_on_a_short_recording_are_still_honoured():
    assert tr.plan(5 * MIN, MIN, 2 * MIN) == tr.Plan("range", MIN, 2 * MIN)


def test_marks_that_cover_everything_count_as_no_marks():
    # The timeline reports its full length as the end until someone moves it.
    assert tr.plan(5 * MIN, 0, 5 * MIN - 30).kind == "whole"


def test_an_unknown_length_is_transcribed_whole():
    assert tr.plan(0, 0, 0) == tr.Plan("whole")


def test_a_marked_range_can_still_be_too_long_to_start_blindly():
    plan = tr.plan(120 * MIN, 0, 90 * MIN)
    assert plan.kind == "range"  # the person chose it; no second guessing


def seg(start, end, text, words=()):
    return CaptionSegment(start_ms=start, end_ms=end, text=text, words=list(words))


def test_shifting_moves_the_cue_and_its_words():
    s = seg(
        1000,
        2000,
        "hei maailma",
        [WordSpan(1000, 1400, "hei"), WordSpan(1400, 2000, " maailma")],
    )
    (moved,) = tr.shifted([s], 600_000)
    assert (moved.start_ms, moved.end_ms) == (601_000, 602_000)
    assert [(w.start_ms, w.end_ms) for w in moved.words] == [
        (601_000, 601_400),
        (601_400, 602_000),
    ]
    assert (s.start_ms, s.end_ms) == (1000, 2000)  # the original is untouched


def test_merging_replaces_only_the_cues_inside_the_range():
    old = [seg(0, 4000, "a"), seg(10_000, 14_000, "b"), seg(30_000, 34_000, "c")]
    new = [seg(11_000, 13_000, "B1"), seg(13_000, 16_000, "B2")]
    merged = tr.merged(old, new, 8_000, 20_000)
    assert [s.text for s in merged] == ["a", "B1", "B2", "c"]
    assert merged == sorted(merged, key=lambda s: s.start_ms)


def test_a_cue_that_straddles_the_start_goes_with_the_range():
    old = [
        seg(0, 4000, "a"),
        seg(7_000, 9_000, "straddles"),
        seg(20_000, 22_000, "after"),
    ]
    merged = tr.merged(old, [seg(8_000, 12_000, "new")], 8_000, 15_000)
    assert [s.text for s in merged] == ["a", "new", "after"]


def test_merging_into_nothing_gives_the_new_cues():
    new = [seg(1, 2, "x")]
    assert tr.merged([], new, 0, 10) == new


def test_minutes_are_written_for_the_prompt():
    assert tr.clock(47 * MIN + 9_000) == "47:09"
    assert tr.clock(75 * MIN) == "1:15:00"


def test_the_ask_text_says_how_long_it_is_and_what_to_do():
    text = tr.ask_text(47 * MIN)
    assert "47" in text and "I" in text and "O" in text
