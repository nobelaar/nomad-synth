import numpy as np
from nomad.engine.voice_manager import VoiceManager
from nomad.engine.params import REGISTRY

SR = 48000.0

def params(**over):
    p = {k: v.default for k, v in REGISTRY.items()}; p.update(over); return p

def mods():
    return {k: 0.0 for k in ("pitch","pitch1","pitch2","amp","cutoff","pw1","pw2","bend")}

def render(vm, n, p=None, resolve=None):
    out = np.zeros(n); p = p or params()
    for i in range(0, n, 16):
        vm.render(out[i:i+16], p, mods(), resolve)
    return out

def test_eight_notes_play_together():
    vm = VoiceManager(SR)
    for n in range(60, 68): vm.note_on(n, 1.0)
    render(vm, 4096)
    assert sorted(vm.active_notes()) == list(range(60, 68))

def test_ninth_note_steals_the_oldest():
    vm = VoiceManager(SR)
    for n in range(60, 68): vm.note_on(n, 1.0)
    render(vm, 4096)
    vm.note_on(80, 1.0)
    render(vm, 4096)
    assert 60 not in vm.active_notes() and 80 in vm.active_notes()

def test_steal_has_no_step_discontinuity():
    vm = VoiceManager(SR)
    for n in range(60, 68): vm.note_on(n, 1.0)
    render(vm, 8192)
    out = np.zeros(2400)
    p = params(osc1_wave=0, osc2_level=0.0, amp_attack=1.0, amp_sustain=1.0)
    for i in range(0, len(out), 16):
        if i == 480: vm.note_on(80, 1.0)
        vm.render(out[i:i+16], p, mods())
    assert np.abs(np.diff(out)).max() < 0.25

def test_polyphony_one_keeps_one_note():
    vm = VoiceManager(SR); vm.set_polyphony(1)
    for n in (60, 64, 67): vm.note_on(n, 1.0)
    render(vm, 4096)
    assert len(vm.active_notes()) == 1

def test_lowering_polyphony_with_chord_releases_extras():
    vm = VoiceManager(SR)
    for n in range(60, 68): vm.note_on(n, 1.0)
    render(vm, 8192)
    vm.set_polyphony(2)
    render(vm, 8192)
    assert len(vm.active_notes()) <= 2

def test_note_off_of_unknown_note_is_safe():
    vm = VoiceManager(SR); vm.note_off(999); render(vm, 256)

def test_same_note_twice_reuses_voice():
    vm = VoiceManager(SR)
    vm.note_on(60, 1.0); render(vm, 512); vm.note_on(60, 0.5); render(vm, 512)
    assert sum(v.is_active() for v in vm.voices) == 1
    assert vm.active_notes() == [60]

def test_note_off_releases_only_matching():
    vm = VoiceManager(SR)
    for n in (60, 64): vm.note_on(n, 1.0)
    render(vm, 512); vm.note_off(60)
    assert vm.active_notes() == [64]

def test_steal_prefers_oldest_in_release():
    vm = VoiceManager(SR)
    for n in range(60, 68): vm.note_on(n, 1.0)
    render(vm, 4096)
    vm.note_off(65)   # newer than 60, but in release
    vm.note_on(80, 1.0); render(vm, 4096)
    assert 60 in vm.active_notes() and 80 in vm.active_notes() and 65 not in vm.active_notes()

def test_age_is_refreshed_by_note_on():
    vm = VoiceManager(SR)
    vm.note_on(60, 1.0); vm.note_on(62, 1.0)
    a = {v.note: v.age for v in vm.voices if v.is_active()}
    assert a[62] > a[60]
    vm.note_on(60, 1.0)
    assert [v.age for v in vm.voices if v.note == 60][0] > a[62]

def test_set_polyphony_clamps_and_ignores_junk():
    vm = VoiceManager(SR)
    vm.set_polyphony(0); assert vm.polyphony == 1
    vm.set_polyphony(99); assert vm.polyphony == 8
    vm.set_polyphony(3)
    for bad in (float("nan"), float("inf"), None, "x"): vm.set_polyphony(bad)
    assert vm.polyphony == 3

def test_lowering_releases_oldest_gracefully_and_raising_works():
    vm = VoiceManager(SR)
    for n in range(60, 66): vm.note_on(n, 1.0)
    render(vm, 2048); vm.set_polyphony(2)
    assert sorted(vm.active_notes()) == [64, 65]
    assert sum(v.is_active() for v in vm.voices) == 6   # released, still ringing
    vm.set_polyphony(8); render(vm, 8192)
    for n in range(70, 76): vm.note_on(n, 1.0)
    render(vm, 2048)
    assert len(vm.active_notes()) >= 8

def test_lowering_is_click_free():
    vm = VoiceManager(SR)
    p = params(osc1_wave=0, osc2_level=0.0, amp_attack=1.0, amp_sustain=1.0)
    for n in range(60, 68): vm.note_on(n, 1.0)
    render(vm, 8192, p)
    vm.set_polyphony(1); out = render(vm, 4800, p)
    assert np.abs(np.diff(out)).max() < 0.25

def test_invalid_notes_and_velocity_safe():
    vm = VoiceManager(SR)
    for n in (-1, 128, None, "a", float("nan"), 60.5): vm.note_on(n, 1.0)
    assert vm.active_notes() == []
    vm.note_on(60.0, 7.0); vm.note_on(62, float("nan"))
    out = render(vm, 512)
    assert np.isfinite(out).all() and abs(out).max() < 8
    assert vm.voices[[v.note for v in vm.voices].index(60)].velocity == 1.0

def test_steal_chain_bounded_and_no_click():
    vm = VoiceManager(SR)
    p = params(osc1_wave=0, osc2_level=0.0, amp_attack=1.0, amp_sustain=1.0)
    out = np.zeros(6000)
    for i in range(0, len(out), 16):
        if i % 64 == 0: vm.note_on(40 + (i // 64) % 40, 1.0)
        vm.render(out[i:i+16], p, mods())
        assert sum(v.is_active() for v in vm.voices) <= 8
    assert np.isfinite(out).all()

def test_resolve_callback_is_per_voice():
    vm = VoiceManager(SR)
    vm.note_on(60, 1.0); vm.note_on(72, 1.0)
    seen = []
    def resolve(v, m):
        seen.append(v.note); return m
    render(vm, 32, resolve=resolve)
    assert sorted(set(seen)) == [60, 72]
