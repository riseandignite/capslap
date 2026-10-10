import json

from app.models.captions import (
    MIN_SEGMENT_MS,
    CaptionSegment,
    CaptionsFile,
    PositionOverride,
    ProjectState,
    WordSpan,
    apply_orphan_rules,
    clamped_segment_time,
    combine_separated_syllables,
    delete_segment,
    group_words_into_phrases,
    set_segment_time,
    shift_word_to_next,
    shift_word_to_prev,
)


def test_captions_file_roundtrip():
    seg1 = CaptionSegment(
        start_ms=1000,
        end_ms=2500,
        text="Hello world",
        words=[
            WordSpan(start_ms=1000, end_ms=1700, text="Hello"),
            WordSpan(start_ms=1700, end_ms=2500, text=" world"),
        ],
    )
    seg2 = CaptionSegment(start_ms=2600, end_ms=4000, text="Second line")
    ov = PositionOverride(start_ms=900, end_ms=3000, y_pct=42.5)

    file_data = CaptionsFile(segments=[seg1, seg2], position_overrides=[ov])
    d = file_data.to_dict()

    assert "segments" in d
    assert "positionOverrides" in d
    assert len(d["segments"]) == 2
    assert d["segments"][0]["startMs"] == 1000
    assert d["segments"][0]["words"][0]["text"] == "Hello"
    assert d["positionOverrides"][0]["yPct"] == 42.5

    # Roundtrip from dict
    restored = CaptionsFile.from_dict(d)
    assert len(restored.segments) == 2
    assert restored.segments[0].text == "Hello world"
    assert len(restored.position_overrides) == 1
    assert restored.position_overrides[0].y_pct == 42.5


def test_load_legacy_bare_array_sidecar():
    legacy_json = json.dumps([{"startMs": 0, "endMs": 1500, "text": "Legacy cue"}])
    loaded = CaptionsFile.from_json_str(legacy_json)
    assert len(loaded.segments) == 1
    assert loaded.segments[0].text == "Legacy cue"
    assert loaded.position_overrides == []


def test_project_state_get_active_segment():
    seg1 = CaptionSegment(start_ms=1000, end_ms=2000, text="First")
    seg2 = CaptionSegment(start_ms=2500, end_ms=3500, text="Second")
    proj = ProjectState(segments=[seg1, seg2])

    assert proj.get_active_segment(500) is None
    assert proj.get_active_segment(1000) == seg1
    assert proj.get_active_segment(1500) == seg1
    assert proj.get_active_segment(2000) == seg1
    assert proj.get_active_segment(2200) is None
    assert proj.get_active_segment(3000) == seg2


def test_position_override_for_segment():
    seg = CaptionSegment(start_ms=1000, end_ms=2000, text="Midpoint is 1500")
    ov1 = PositionOverride(
        start_ms=500, end_ms=1200, y_pct=20.0
    )  # does not cover midpoint 1500
    ov2 = PositionOverride(
        start_ms=1200, end_ms=1800, y_pct=75.0
    )  # covers midpoint 1500

    proj = ProjectState(segments=[seg], position_overrides=[ov1, ov2])
    # Default is 80.0
    assert proj.get_anchor_y_for_segment(seg, default_y=80.0) == 75.0

    # Without matching override
    proj.position_overrides = [ov1]
    assert proj.get_anchor_y_for_segment(seg, default_y=80.0) == 80.0


def test_set_override_for_active_segment():
    seg = CaptionSegment(start_ms=1000, end_ms=2000, text="Test")
    proj = ProjectState(segments=[seg])
    assert proj.is_dirty is False

    proj.set_segment_position_override(seg, 35.0)
    assert proj.is_dirty is True
    assert len(proj.position_overrides) == 1
    assert proj.position_overrides[0].y_pct == 35.0
    assert proj.position_overrides[0].start_ms == 1000
    assert proj.position_overrides[0].end_ms == 2000


def test_caption_segment_update_text_synchronizes_words():
    seg = CaptionSegment(
        start_ms=1000,
        end_ms=3000,
        text="Original words here",
        words=[
            WordSpan(start_ms=1000, end_ms=1600, text="Original"),
            WordSpan(start_ms=1600, end_ms=2300, text=" words"),
            WordSpan(start_ms=2300, end_ms=3000, text=" here"),
        ],
    )

    # 1. Edit with same word count: preserves timings
    seg.update_text("Updated words now")
    assert seg.text == "Updated words now"
    assert len(seg.words) == 3
    assert seg.words[0].text == "Updated"
    assert seg.words[0].start_ms == 1000
    assert seg.words[0].end_ms == 1600
    assert seg.words[2].text == " now"
    assert seg.words[2].end_ms == 3000

    # 2. Edit with different word count: re-interpolates timings
    seg.update_text("Completely rewritten sentence for testing")
    assert seg.text == "Completely rewritten sentence for testing"
    assert len(seg.words) == 5
    assert seg.words[0].text == "Completely"
    assert seg.words[0].start_ms == 1000
    assert seg.words[-1].end_ms == 3000

    # 3. Serialization to_dict includes updated words
    d = seg.to_dict()
    assert d["text"] == "Completely rewritten sentence for testing"
    assert len(d["words"]) == 5
    assert d["words"][0]["text"] == "Completely"


def test_combine_separated_syllables_within_segment():
    # Syllables with trailing hyphens or glue_to_previous should merge into one word
    seg = CaptionSegment(
        start_ms=0,
        end_ms=2000,
        text="sep- a- rat- ed syllables",
        words=[
            WordSpan(start_ms=0, end_ms=300, text="sep-"),
            WordSpan(start_ms=300, end_ms=600, text="a-"),
            WordSpan(start_ms=600, end_ms=900, text="rat-"),
            WordSpan(start_ms=900, end_ms=1300, text="ed"),
            WordSpan(start_ms=1400, end_ms=2000, text="syllables"),
        ],
    )
    result = combine_separated_syllables([seg])
    assert len(result) == 1
    assert len(result[0].words) == 2
    assert result[0].words[0].text == "separated"
    assert result[0].words[0].start_ms == 0
    assert result[0].words[0].end_ms == 1300
    assert result[0].words[1].text == "syllables"
    assert result[0].text == "separated syllables"


def test_combine_separated_syllables_with_glue_to_previous():
    # Finnish word pieces with glue_to_previous
    seg = CaptionSegment(
        start_ms=0,
        end_ms=2000,
        text="kaup pa kes kus",
        words=[
            WordSpan(start_ms=0, end_ms=400, text="kaup"),
            WordSpan(start_ms=400, end_ms=800, text="pa", glue_to_previous=True),
            WordSpan(start_ms=800, end_ms=1200, text="kes", glue_to_previous=True),
            WordSpan(start_ms=1200, end_ms=1800, text="kus", glue_to_previous=True),
        ],
    )
    result = combine_separated_syllables([seg])
    assert len(result) == 1
    assert len(result[0].words) == 1
    assert result[0].words[0].text == "kauppakeskus"
    assert result[0].words[0].start_ms == 0
    assert result[0].words[0].end_ms == 1800
    assert result[0].text == "kauppakeskus"


def test_combine_separated_syllables_across_segment_boundaries():
    # Word broken across segment boundary: seg0 ends with 'korke', seg1 starts with 'alla' (glue)
    seg1 = CaptionSegment(
        start_ms=0,
        end_ms=1000,
        text="tarpeeksi korke",
        words=[
            WordSpan(start_ms=0, end_ms=500, text="tarpeeksi"),
            WordSpan(start_ms=500, end_ms=1000, text="korke"),
        ],
    )
    seg2 = CaptionSegment(
        start_ms=1000,
        end_ms=2500,
        text="alla birtsille",
        words=[
            WordSpan(start_ms=1000, end_ms=1500, text="alla", glue_to_previous=True),
            WordSpan(start_ms=1500, end_ms=2500, text="birtsille"),
        ],
    )
    result = combine_separated_syllables([seg1, seg2])
    assert len(result) == 2
    # seg1 ends with full word 'korkealla'
    assert result[0].text == "tarpeeksi korkealla"
    assert result[0].words[-1].text == "korkealla"
    assert result[0].words[-1].end_ms == 1500
    assert result[0].end_ms == 1500
    # seg2 starts with 'birtsille'
    assert result[1].text == "birtsille"
    assert result[1].words[0].text == "birtsille"
    assert result[1].start_ms == 1500


def test_shift_word_to_prev_and_next():
    seg1 = CaptionSegment(
        start_ms=0,
        end_ms=1000,
        text="Hello",
        words=[WordSpan(start_ms=0, end_ms=1000, text="Hello")],
    )
    seg2 = CaptionSegment(
        start_ms=1100,
        end_ms=2500,
        text="world again",
        words=[
            WordSpan(start_ms=1100, end_ms=1800, text="world"),
            WordSpan(start_ms=1900, end_ms=2500, text="again"),
        ],
    )
    segments = [seg1, seg2]

    # Shift 'world' from seg2 to seg1
    shift_word_to_prev(segments, 1)
    assert len(segments[0].words) == 2
    assert segments[0].text == "Hello world"
    assert segments[0].end_ms == 1800
    assert len(segments[1].words) == 1
    assert segments[1].text == "again"
    assert segments[1].start_ms == 1900

    # Shift 'world' back from seg1 to seg2
    shift_word_to_next(segments, 0)
    assert len(segments[0].words) == 1
    assert segments[0].text == "Hello"
    assert segments[0].end_ms == 1000
    assert len(segments[1].words) == 2
    assert segments[1].text == "world again"
    assert segments[1].start_ms == 1100


def test_shift_word_to_prev_combines_glued_syllable():
    # 'pa' is a syllable glued to whatever word precedes it; shifting it into
    # the previous segment must combine it into that word, not add a space.
    seg1 = CaptionSegment(
        start_ms=0,
        end_ms=1000,
        text="kaup",
        words=[WordSpan(start_ms=0, end_ms=1000, text="kaup")],
    )
    seg2 = CaptionSegment(
        start_ms=1000,
        end_ms=2000,
        text="pa kesä",
        words=[
            WordSpan(start_ms=1000, end_ms=1500, text="pa", glue_to_previous=True),
            WordSpan(start_ms=1500, end_ms=2000, text="kesä"),
        ],
    )
    segments = [seg1, seg2]

    shift_word_to_prev(segments, 1)
    assert segments[0].text == "kauppa"
    assert segments[1].text == "kesä"


def test_shift_word_to_next_combines_glued_syllable():
    # The word remaining first in the next segment glues to whatever now
    # precedes it; shifting a word in front of it must combine, not space.
    seg1 = CaptionSegment(
        start_ms=0,
        end_ms=1000,
        text="kaup",
        words=[WordSpan(start_ms=0, end_ms=1000, text="kaup")],
    )
    seg2 = CaptionSegment(
        start_ms=1000,
        end_ms=2000,
        text="pa",
        words=[WordSpan(start_ms=1000, end_ms=2000, text="pa", glue_to_previous=True)],
    )
    segments = [seg1, seg2]

    shift_word_to_next(segments, 0)
    assert segments[0].text == ""
    assert segments[1].text == "kauppa"


def test_caption_segment_update_text_when_words_empty():
    seg = CaptionSegment(start_ms=1000, end_ms=3000, text="Initial", words=[])
    seg.update_text("Three new words")
    assert seg.text == "Three new words"
    assert len(seg.words) == 3
    assert seg.words[0].text == "Three"
    assert seg.words[0].start_ms == 1000
    assert seg.words[-1].end_ms == 3000


def test_apply_orphan_rules_trailing_conjunction():
    seg1 = CaptionSegment(
        start_ms=0,
        end_ms=2000,
        text="Tämä on ensimmäinen lause ja",
        words=[
            WordSpan(start_ms=0, end_ms=500, text="Tämä"),
            WordSpan(start_ms=500, end_ms=800, text=" on"),
            WordSpan(start_ms=800, end_ms=1500, text=" ensimmäinen"),
            WordSpan(start_ms=1500, end_ms=1800, text=" lause"),
            WordSpan(start_ms=1800, end_ms=2000, text=" ja"),
        ],
    )
    seg2 = CaptionSegment(
        start_ms=2100,
        end_ms=4000,
        text="toinen lause tässä",
        words=[
            WordSpan(start_ms=2100, end_ms=2700, text="toinen"),
            WordSpan(start_ms=2700, end_ms=3300, text=" lause"),
            WordSpan(start_ms=3300, end_ms=4000, text=" tässä"),
        ],
    )
    results = apply_orphan_rules([seg1, seg2])
    assert len(results) == 2
    assert results[0].text == "Tämä on ensimmäinen lause"
    assert results[0].end_ms == 1800
    assert len(results[0].words) == 4
    assert results[1].text == "ja toinen lause tässä"
    assert results[1].start_ms == 1800
    assert len(results[1].words) == 4
    assert results[1].words[0].text == "ja"


def test_apply_orphan_rules_post_comma_single_word():
    seg1 = CaptionSegment(
        start_ms=0,
        end_ms=2500,
        text="Tämä on tärkeää, totta",
        words=[
            WordSpan(start_ms=0, end_ms=500, text="Tämä"),
            WordSpan(start_ms=500, end_ms=900, text=" on"),
            WordSpan(start_ms=900, end_ms=1800, text=" tärkeää,"),
            WordSpan(start_ms=1800, end_ms=2500, text=" totta"),
        ],
    )
    seg2 = CaptionSegment(
        start_ms=2600,
        end_ms=5000,
        text="kai me se tiedämme",
        words=[
            WordSpan(start_ms=2600, end_ms=3200, text="kai"),
            WordSpan(start_ms=3200, end_ms=4000, text=" me"),
            WordSpan(start_ms=4000, end_ms=4500, text=" se"),
            WordSpan(start_ms=4500, end_ms=5000, text=" tiedämme"),
        ],
    )
    results = apply_orphan_rules([seg1, seg2])
    assert len(results) == 2
    assert results[0].text == "Tämä on tärkeää,"
    assert results[0].end_ms == 1800
    assert len(results[0].words) == 3
    assert results[1].text == "totta kai me se tiedämme"
    assert results[1].start_ms == 1800
    assert results[1].words[0].text == "totta"


def test_apply_orphan_rules_last_segment_not_broken():
    seg = CaptionSegment(start_ms=0, end_ms=1000, text="Viimeinen ja")
    results = apply_orphan_rules([seg])
    assert len(results) == 1
    assert results[0].text == "Viimeinen ja"


def test_apply_orphan_rules_one_word_segment_not_emptied():
    seg1 = CaptionSegment(start_ms=0, end_ms=500, text="ja")
    seg2 = CaptionSegment(start_ms=600, end_ms=1200, text="sitten")
    results = apply_orphan_rules([seg1, seg2])
    assert results[0].text == "ja"
    assert results[1].text == "sitten"


def test_delete_segment_gives_time_to_previous_cue():
    seg1 = CaptionSegment(
        start_ms=0,
        end_ms=1000,
        text="First",
        words=[WordSpan(0, 1000, "First")],
    )
    seg2 = CaptionSegment(start_ms=1000, end_ms=2500, text="", words=[])
    seg3 = CaptionSegment(
        start_ms=2500,
        end_ms=3000,
        text="Third",
        words=[WordSpan(2500, 3000, "Third")],
    )
    segments = [seg1, seg2, seg3]

    delete_segment(segments, 1)

    assert len(segments) == 2
    assert segments[0].end_ms == 2500
    # The burner takes cue times from the words, so the tail word has to follow.
    assert segments[0].words[-1].end_ms == 2500
    assert segments[1].start_ms == 2500


def test_delete_first_segment_stretches_the_next_one_backwards():
    seg1 = CaptionSegment(start_ms=200, end_ms=1000, text="", words=[])
    seg2 = CaptionSegment(
        start_ms=1000,
        end_ms=2000,
        text="Second",
        words=[WordSpan(1000, 2000, "Second")],
    )
    segments = [seg1, seg2]

    delete_segment(segments, 0)

    assert len(segments) == 1
    assert segments[0].start_ms == 200
    assert segments[0].words[0].start_ms == 200


def test_set_segment_time_scales_word_timings():
    seg = CaptionSegment(
        start_ms=1000,
        end_ms=2000,
        text="Yksi kaksi",
        words=[WordSpan(1000, 1500, "Yksi"), WordSpan(1500, 2000, "kaksi")],
    )

    # Same length, moved one second later.
    set_segment_time(seg, 2000, 3000)
    assert (seg.start_ms, seg.end_ms) == (2000, 3000)
    assert [(w.start_ms, w.end_ms) for w in seg.words] == [(2000, 2500), (2500, 3000)]

    # Stretched to double length: the split stays proportional.
    set_segment_time(seg, 2000, 4000)
    assert [(w.start_ms, w.end_ms) for w in seg.words] == [(2000, 3000), (3000, 4000)]


def test_clamped_segment_time_respects_neighbours_and_minimum():
    segments = [
        CaptionSegment(start_ms=0, end_ms=1000, text="a"),
        CaptionSegment(start_ms=1000, end_ms=2000, text="b"),
        CaptionSegment(start_ms=2000, end_ms=3000, text="c"),
    ]

    # Dragging the start before the previous cue's end is not allowed.
    assert clamped_segment_time(segments, 1, 500, 2000) == (1000, 2000)
    # Nor is running past the next cue's start.
    assert clamped_segment_time(segments, 1, 1000, 2500) == (1000, 2000)
    # A cue can never collapse to nothing.
    start, end = clamped_segment_time(segments, 1, 1990, 1995)
    assert end - start >= MIN_SEGMENT_MS


def test_group_words_into_phrases_merges_word_per_cue_transcripts():
    # What karaoke transcription used to produce: one cue per word.
    words = [
        ("Moi", 0, 400),
        ("täällä", 500, 900),
        ("Mikko", 950, 1400),
        ("Hyppönen.", 1450, 2000),
        ("Vuonna", 3000, 3400),
        ("2026", 3450, 3900),
    ]
    segments = [
        CaptionSegment(start_ms=s, end_ms=e, text=t, words=[WordSpan(s, e, t)])
        for t, s, e in words
    ]

    grouped = group_words_into_phrases(segments)

    assert [g.text for g in grouped] == [
        "Moi täällä Mikko Hyppönen.",
        "Vuonna 2026",
    ]
    # Cue times and word timings survive the merge, so karaoke still lines up.
    assert (grouped[0].start_ms, grouped[0].end_ms) == (0, 2000)
    assert [w.text for w in grouped[0].words] == ["Moi", "täällä", "Mikko", "Hyppönen."]
    assert grouped[1].start_ms == 3000


def test_group_words_into_phrases_rejoins_split_words():
    segments = [
        CaptionSegment(
            start_ms=0, end_ms=400, text="Kyber-", words=[WordSpan(0, 400, "Kyber-")]
        ),
        CaptionSegment(
            start_ms=400,
            end_ms=900,
            text="turvallisuudesta",
            words=[WordSpan(400, 900, "turvallisuudesta")],
        ),
        CaptionSegment(
            start_ms=950,
            end_ms=1300,
            text="pa",
            words=[WordSpan(950, 1300, "pa", glue_to_previous=True)],
        ),
    ]

    grouped = group_words_into_phrases(segments)

    assert len(grouped) == 1
    assert grouped[0].text == "Kyberturvallisuudestapa"


def test_group_words_into_phrases_breaks_on_pauses_and_length():
    # A long pause starts a new cue even mid-sentence.
    segments = [
        CaptionSegment(
            start_ms=0, end_ms=400, text="yksi", words=[WordSpan(0, 400, "yksi")]
        ),
        CaptionSegment(
            start_ms=2000,
            end_ms=2400,
            text="kaksi",
            words=[WordSpan(2000, 2400, "kaksi")],
        ),
    ]
    assert len(group_words_into_phrases(segments)) == 2

    # And a cue never grows past the word limit.
    many = [
        CaptionSegment(
            start_ms=i * 100,
            end_ms=i * 100 + 90,
            text="ok",
            words=[WordSpan(i * 100, i * 100 + 90, "ok")],
        )
        for i in range(10)
    ]
    grouped = group_words_into_phrases(many, max_words=4)
    assert all(len(g.words) <= 4 for g in grouped)
