import numpy as np

from nomad.engine.engine import Engine


def make():
    e = Engine(48000)
    e.seq_clock.bpm = 120.0  # 6000 frames per 16th
    s = e.synth
    s.sequencer.on.fill(0)
    s.set_rec(True)
    return e, s, s.sequencer


def render(e, n):
    e.render(np.zeros(n))


def test_step_record_writes_and_advances_with_wrap():
    e, s, sq = make()
    for n in range(60, 77):  # 17 notes: wraps to step 1
        s.note_on(n, 0.8)
        s.note_off(n)  # the cursor moves when the key comes up
    render(e, 64)
    assert list(sq.note[:16]) == [76] + list(range(61, 76))
    assert sq.on.all() and sq.rec_step == 1


def test_rest_skips_a_step():
    e, s, sq = make()
    s.note_on(60, 1.0)
    s.note_off(60)
    s.rest()
    s.note_on(62, 1.0)
    render(e, 64)
    assert list(sq.on[:3]) == [1, 0, 1] and sq.note[2] == 62


def test_realtime_quantizes_to_nearest_step():
    e, s, sq = make()
    s.start()
    render(e, 6000 + 4000)  # 1.67 steps in: nearest is step 2
    s.note_on(65, 1.0)
    render(e, 64)
    assert sq.on[2] == 1 and sq.note[2] == 65 and sq.on.sum() == 1


def test_rec_off_does_not_write():
    e, s, sq = make()
    s.set_rec(False)
    s.note_on(60, 1.0)
    render(e, 64)
    assert sq.on.sum() == 0


def test_chord_recorded_from_held_keys_in_one_step():
    e, s, sq = make()
    for n in (60, 64, 67):
        s.note_on(n, 1.0)
    for n in (60, 64, 67):
        s.note_off(n)
    s.note_on(62, 1.0)
    s.note_off(62)
    render(e, 64)
    assert sq.chord(0) == [60, 64, 67] and sq.chord(1) == [62] and sq.rec_step == 2


def test_chord_plays_all_notes_and_releases_them():
    e, s, sq = make()
    sq.set_step(0, True, note=60)
    for n in (64, 67):
        sq.add_note(0, n)
    sq.enabled, sq.gate = True, 0.5
    s.start()
    s.set_rec(False)
    render(e, 100)
    assert sorted(s.vm.active_notes()) == [60, 64, 67]
    render(e, 4000)  # past the gate (3000 frames)
    assert s.vm.active_notes() == []


def test_realtime_chord_and_rewrite_on_next_pass():
    e, s, sq = make()
    s.start()
    render(e, 100)
    s.note_on(60, 1.0)
    s.note_on(64, 1.0)
    render(e, 64)
    assert sq.chord(0) == [60, 64]
    render(e, 6000 * 16)  # a full pass later, same step: first note rewrites
    s.note_on(67, 1.0)
    render(e, 64)
    assert sq.chord(0) == [67]


def test_transpose_moves_whole_chord_and_clamps():
    e, s, sq = make()
    sq.set_step(0, True, note=60)
    sq.add_note(0, 127)
    sq.transpose(0, 12)
    assert sq.chord(0) == [60, 127]
    sq.transpose(0, -12)
    assert sq.chord(0) == [48, 115]


def test_div_bar_len_4_walks_four_chords():
    e, s, sq = make()
    s.set_rec(False)
    sq.grid.division, sq.grid.length, sq.enabled, sq.gate = 4.0, 4, True, 1.0
    for i, n in enumerate((60, 57, 53, 55)):
        sq.set_step(i, True, note=n)
    s.start()
    seen = []
    render(e, 48000)  # mid first bar (one bar at 120 bpm = 2 s)
    for _ in range(5):
        seen.append(s.vm.active_notes())
        render(e, 96000)
    assert [a[0] for a in seen] == [60, 57, 53, 55, 60]
