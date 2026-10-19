// The chime for a new chat message or friend request (D-064): two quick bell
// notes, like a note block's "pling", made here with Web Audio (no sound file).

let context: AudioContext | null = null;

function audio(): AudioContext | null {
  if (typeof AudioContext === 'undefined') return null;
  context ??= new AudioContext();
  return context;
}

/**
 * Browsers start sound only after the player touched the page: the first
 * click or key wakes the audio up for later chimes.
 */
export function unlockSoundOnFirstInput(): () => void {
  const wake = () => {
    void audio()
      ?.resume()
      .catch(() => undefined);
  };
  window.addEventListener('pointerdown', wake, { once: true });
  window.addEventListener('keydown', wake, { once: true });
  return () => {
    window.removeEventListener('pointerdown', wake);
    window.removeEventListener('keydown', wake);
  };
}

/** One bell note: a sine with a soft octave above, a quick start and a long fade. */
function note(ctx: AudioContext, frequency: number, start: number, volume: number) {
  const gain = ctx.createGain();
  gain.gain.setValueAtTime(0.0001, start);
  gain.gain.exponentialRampToValueAtTime(volume, start + 0.008);
  gain.gain.exponentialRampToValueAtTime(0.0001, start + 0.55);
  gain.connect(ctx.destination);
  for (const [type, ratio, level] of [
    ['sine', 1, 1],
    ['triangle', 2, 0.18],
  ] as const) {
    const osc = ctx.createOscillator();
    const partial = ctx.createGain();
    osc.type = type;
    osc.frequency.setValueAtTime(frequency * ratio, start);
    partial.gain.value = level;
    osc.connect(partial).connect(gain);
    osc.start(start);
    osc.stop(start + 0.6);
  }
}

/** Plays the chime; silently nothing when the system gives no sound. */
export function playChime(): void {
  const ctx = audio();
  if (ctx === null) return;
  const play = () => {
    const now = ctx.currentTime + 0.01;
    // F♯6 then B6: the bright, rising pair a note block plays.
    note(ctx, 1479.98, now, 0.12);
    note(ctx, 1975.53, now + 0.085, 0.1);
  };
  if (ctx.state === 'suspended') {
    void ctx
      .resume()
      .then(play)
      .catch(() => undefined);
  } else {
    play();
  }
}
