// The 3D character (skinview3d on three.js, both bundled with the app: nothing is
// loaded from the network). Textures come in as canvases cut from the core's
// `data:` URLs; their frames change here by the wall clock, as in the game.
import type { PlayerAnimation, SkinViewer } from 'skinview3d';
import { frameAt } from './textures';

export type BodyAnimation = 'idle' | 'walk' | 'run' | 'wave' | 'crouch' | 'hit' | 'fly';
export const BODY_ANIMATIONS: readonly BodyAnimation[] = [
  'idle',
  'walk',
  'run',
  'wave',
  'crouch',
  'hit',
  'fly',
];

export type ViewAngle = 'front' | 'threeQuarter' | 'back' | 'spin';
export const VIEW_ANGLES: readonly ViewAngle[] = ['front', 'threeQuarter', 'back', 'spin'];

const ANGLES: Record<Exclude<ViewAngle, 'spin'>, number> = {
  front: 0,
  threeQuarter: Math.PI / 5,
  back: Math.PI,
};

/** A texture ready for the character: its frames and how long each shows. */
export interface Frames {
  /** Changes when the texture does, so frames are only swapped when needed. */
  key: string;
  frames: readonly HTMLCanvasElement[];
  durations: readonly number[];
}

export interface Scene {
  skin: Frames | null;
  slim: boolean;
  cape: Frames | null;
  back: 'cape' | 'elytra';
  animation: BodyAnimation;
  layer2: boolean;
  nameTag: string | null;
  /** A frame held still (paused in the frame editor); null follows the clock. */
  frame: number | null;
  /** False with the OS setting "reduce motion": nothing moves by itself. */
  moving: boolean;
}

type Library = typeof import('skinview3d');

/** Whether this webview can draw WebGL at all; asked before loading three.js. */
export function webglAvailable(): boolean {
  if (typeof WebGLRenderingContext === 'undefined') return false;
  try {
    const canvas = document.createElement('canvas');
    return (canvas.getContext('webgl2') ?? canvas.getContext('webgl')) !== null;
  } catch {
    return false;
  }
}

function animationOf(lib: Library, id: BodyAnimation): PlayerAnimation {
  switch (id) {
    case 'walk':
      return new lib.WalkingAnimation();
    case 'run':
      return new lib.RunningAnimation();
    case 'wave':
      return new lib.WaveAnimation();
    case 'crouch':
      return new lib.CrouchAnimation();
    case 'hit':
      return new lib.HitAnimation();
    case 'fly':
      return new lib.FlyingAnimation();
    case 'idle':
      return new lib.IdleAnimation();
  }
}

/** One character on one canvas. */
export class Stage3D {
  private readonly lib: Library;
  private readonly viewer: SkinViewer;
  private scene: Scene | null = null;
  private view: ViewAngle = 'spin';
  private hovered = false;
  private shownSkin = '';
  private shownCape = '';
  private readonly timer: ReturnType<typeof setInterval>;
  private readonly onVisibility = () => {
    this.viewer.renderPaused = document.hidden;
  };

  private constructor(lib: Library, canvas: HTMLCanvasElement, width: number, height: number) {
    this.lib = lib;
    this.viewer = new lib.SkinViewer({ canvas, width, height, zoom: 0.7 });
    this.viewer.autoRotateSpeed = 0.6;
    this.timer = setInterval(() => {
      this.showFrames(false);
    }, 40);
    document.addEventListener('visibilitychange', this.onVisibility);
    this.onVisibility();
  }

  /** Loads three.js and makes the character; throws when WebGL does not start. */
  static async create(canvas: HTMLCanvasElement, width: number, height: number): Promise<Stage3D> {
    const lib = await import('skinview3d');
    return new Stage3D(lib, canvas, width, height);
  }

  /** Shows a scene; only what changed is redone. */
  set(next: Scene) {
    const before = this.scene;
    this.scene = next;
    const v = this.viewer;
    if (before?.animation !== next.animation) {
      v.animation = animationOf(this.lib, next.animation);
    }
    if (v.animation !== null) v.animation.paused = !next.moving;
    if (before?.nameTag !== next.nameTag) {
      v.nameTag =
        next.nameTag === null
          ? null
          : new this.lib.NameTagObject(next.nameTag, { font: '48px system-ui, sans-serif' });
    }
    const forceSkin = before?.slim !== next.slim || before.skin?.key !== next.skin?.key;
    const forceCape =
      before?.back !== next.back ||
      before.animation !== next.animation ||
      before.cape?.key !== next.cape?.key;
    this.showFrames(forceSkin, forceCape);
    if (before?.layer2 !== next.layer2 || forceSkin) {
      v.playerObject.skin.setOuterLayerVisible(next.layer2);
    }
    this.applyRotation();
  }

  setView(view: ViewAngle) {
    this.view = view;
    if (view !== 'spin') {
      this.viewer.playerWrapper.rotation.y = ANGLES[view];
      this.viewer.resetCameraPose();
    }
    this.applyRotation();
  }

  /** Turns the character by a step (the arrow keys); it stops spinning. */
  turn(radians: number) {
    this.view = 'front';
    this.viewer.playerWrapper.rotation.y += radians;
    this.applyRotation();
  }

  /** The camera as it was, the character facing it (a double click). */
  reset() {
    this.viewer.resetCameraPose();
    if (this.view !== 'spin') this.viewer.playerWrapper.rotation.y = ANGLES[this.view];
  }

  /** The pointer over the character stops it spinning. */
  hover(over: boolean) {
    this.hovered = over;
    this.applyRotation();
  }

  setSize(width: number, height: number) {
    this.viewer.setSize(width, height);
  }

  /** A PNG of the character as it stands now, `size` pixels square, with a
   * transparent background. Drawn and read in one go, so the page never shows the
   * larger canvas. */
  snapshot(size: number): string {
    const v = this.viewer;
    const { width, height, pixelRatio } = v;
    v.pixelRatio = 1;
    v.setSize(size, size);
    v.render();
    const url = v.canvas.toDataURL('image/png');
    v.pixelRatio = pixelRatio;
    v.setSize(width, height);
    v.render();
    return url;
  }

  /** Whether the WebGL context was lost (a driver reset, sleep). */
  get lost(): boolean {
    return this.viewer.renderer.getContext().isContextLost();
  }

  dispose() {
    clearInterval(this.timer);
    document.removeEventListener('visibilitychange', this.onVisibility);
    this.viewer.dispose();
  }

  private applyRotation() {
    const moving = this.scene?.moving ?? true;
    this.viewer.autoRotate = this.view === 'spin' && moving && !this.hovered;
  }

  /** Puts the frame due now on the character, when it is not there already. */
  private showFrames(forceSkin: boolean, forceCape = forceSkin) {
    const scene = this.scene;
    if (scene === null) return;
    const now = Date.now();
    const pick = (texture: Frames) =>
      scene.frame !== null
        ? Math.min(Math.max(0, scene.frame), texture.frames.length - 1)
        : scene.moving
          ? frameAt(texture.durations, now)
          : 0;
    const v = this.viewer;
    if (scene.skin === null) {
      if (forceSkin || this.shownSkin !== '') v.loadSkin(null);
      this.shownSkin = '';
    } else {
      const index = pick(scene.skin);
      const key = `${scene.skin.key}:${String(index)}`;
      const frame = scene.skin.frames[index];
      if (frame !== undefined && (forceSkin || key !== this.shownSkin)) {
        v.loadSkin(frame, { model: scene.slim ? 'slim' : 'default' });
        v.playerObject.skin.setOuterLayerVisible(scene.layer2);
        this.shownSkin = key;
      }
    }
    // Flying is always on elytra, as in the game.
    const back = scene.animation === 'fly' ? 'elytra' : scene.back;
    if (scene.cape === null) {
      if (forceCape || this.shownCape !== '') v.loadCape(null);
      this.shownCape = '';
    } else {
      const index = pick(scene.cape);
      const key = `${scene.cape.key}:${String(index)}:${back}`;
      const frame = scene.cape.frames[index];
      if (frame !== undefined && (forceCape || key !== this.shownCape)) {
        v.loadCape(frame, { backEquipment: back });
        this.shownCape = key;
      }
    }
  }
}
