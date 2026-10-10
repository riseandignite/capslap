import json
from pathlib import Path

import pytest

from app.models.captions import CaptionSegment, ProjectState, WordSpan
from app.models.project import VideoMetadata
from app.models.review import (
    ReviewMismatch,
    WrongPassword,
    apply_review,
    is_locked,
    lock_file,
    new_review_password,
    review_export_dict,
    unlock_file,
)

# Locked with the same code; the review page's tests open it too (review/lock.test.js).
LOCKED_FIXTURE = (
    Path(__file__).parents[2] / "review" / "testdata" / "locked.capslap.json"
)


def _segments():
    return [
        CaptionSegment(
            0,
            2000,
            "Kylläpä on sää",
            words=[
                WordSpan(0, 600, "Kylläpä"),
                WordSpan(600, 1200, " on"),
                WordSpan(1200, 2000, " sää"),
            ],
        ),
        CaptionSegment(2000, 4000, "tekoäly-"),
        CaptionSegment(4500, 6000, "yhtiöt"),
    ]


def _reviewed(segments, **review):
    data = {"segments": [s.to_dict() for s in segments]}
    if review:
        data["review"] = review
    return json.loads(json.dumps(data))


def test_export_adds_the_video_and_stays_a_sidecar():
    project = ProjectState(
        video=VideoMetadata(
            path="/clips/talk.mp4",
            width=1080,
            height=1920,
            duration_sec=40.3,
            fps=30,
            codec="h264",
        ),
        segments=_segments(),
    )
    data = review_export_dict(project)
    assert data["video"] == {"name": "talk.mp4", "durationMs": 40300}
    assert len(data["segments"]) == 3
    assert "style" in data and "positionOverrides" in data


def test_untouched_review_changes_nothing():
    segments = _segments()
    result = apply_review(
        segments, _reviewed(_segments(), status="approved", reviewer="Asiakas")
    )
    assert not result.changed
    assert result.summary() == "Review from Asiakas: approved, no changes."
    assert [s.to_dict() for s in segments] == [s.to_dict() for s in _segments()]


def test_text_fix_keeps_our_word_timings():
    segments = _segments()
    edited = _segments()
    edited[0].text = "Kyllä on sää"
    edited[0].words = []  # the page's words are ignored either way
    result = apply_review(segments, _reviewed(edited, status="changed"))

    assert result.text_changes == 1 and result.time_changes == 0
    assert segments[0].text == "Kyllä on sää"
    assert [(w.start_ms, w.end_ms) for w in segments[0].words] == [
        (0, 600),
        (600, 1200),
        (1200, 2000),
    ]
    assert segments[0].words[0].text == "Kyllä"


def test_time_fix_moves_the_cue_and_its_words():
    segments = _segments()
    edited = _segments()
    edited[0].start_ms, edited[0].end_ms = 200, 1800
    result = apply_review(segments, _reviewed(edited))
    assert result.time_changes == 1
    assert (segments[0].start_ms, segments[0].end_ms) == (200, 1800)
    assert segments[0].words[0].start_ms == 200
    assert segments[0].words[-1].end_ms == 1800


def test_comments_come_back_with_their_caption():
    result = apply_review(
        _segments(),
        _reviewed(
            _segments(),
            reviewer=" Asiakas ",
            comments=[
                {"index": 2, "text": " Nimi väärin? "},
                {"index": 9, "text": "out of range"},
                {"index": 0, "text": "  "},
            ],
        ),
    )
    assert result.comments == [(3, "yhtiöt", "Nimi väärin?")]
    assert result.summary() == "Review from Asiakas: 1 comment(s)."


def test_a_review_of_other_captions_is_refused():
    segments = _segments()
    try:
        apply_review(segments, _reviewed(_segments()[:2]))
    except ReviewMismatch as err:
        assert "2 captions" in str(err) and "3" in str(err)
    else:
        raise AssertionError("expected ReviewMismatch")
    assert [s.text for s in segments] == [s.text for s in _segments()]


def test_a_locked_file_hides_the_captions_and_opens_with_the_password():
    data = {"segments": [{"startMs": 0, "endMs": 1000, "text": "Salainen sää"}]}
    locked = lock_file(data, "k7mq-x2fp", iterations=1000)
    assert is_locked(locked) and not is_locked(data)
    assert "Salainen" not in json.dumps(locked)
    assert unlock_file(locked, "k7mq-x2fp") == data
    with pytest.raises(WrongPassword):
        unlock_file(locked, "k7mq-x2fq")


def test_opens_the_file_the_review_page_tests_open():
    data = unlock_file(json.loads(LOCKED_FIXTURE.read_text()), "k7mq-x2fp-9tza-hw4c")
    assert data["segments"][1]["text"] == "tekoäly yhtiöt"


def test_a_damaged_locked_file_is_not_a_wrong_password():
    locked = lock_file({"segments": []}, "pw", iterations=1000)
    locked["iv"] = "not base64!"
    with pytest.raises(ValueError) as err:
        unlock_file(locked, "pw")
    assert not isinstance(err.value, WrongPassword)


def test_new_passwords_are_readable_and_differ():
    a, b = new_review_password(), new_review_password()
    assert a != b
    assert len(a) == 19 and a.count("-") == 3
    assert not set(a) & set("01ilo")
