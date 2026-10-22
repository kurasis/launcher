import { strings } from '../../lib/strings';
import { type Texture, dataUrlBytes, frameDurations, frameHeight } from './textures';

/** One line about a texture: size, HD, frames, cycle, weight. */
export function factsOf(texture: Texture, bytes?: number): string {
  const f = strings.accounts.skins.facts;
  const height = frameHeight(texture);
  const parts = [f.size({ width: texture.width, height })];
  if (texture.width > 64) parts.push(f.hd({ scale: texture.width / 64 }));
  parts.push(f.frames({ count: texture.frames }));
  if (texture.frames > 1) {
    const total = frameDurations(texture.frames, texture.frameMs).reduce((s, ms) => s + ms, 0);
    parts.push(f.cycle({ seconds: Math.round(total / 100) / 10 }));
  }
  const weight = bytes ?? dataUrlBytes(texture.src);
  parts.push(f.kb({ kb: Math.max(1, Math.round(weight / 1024)) }));
  return parts.join(' · ');
}
