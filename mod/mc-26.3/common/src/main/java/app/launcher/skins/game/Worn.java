package app.launcher.skins.game;

import app.launcher.skins.core.Texture;
import net.minecraft.resources.Identifier;

/** A player's skin and cape as registered textures, one per frame. */
final class Worn {
    final Texture skin;
    final Identifier[] skinFrames;
    final Texture cape;
    final Identifier[] capeFrames;

    Worn(Texture skin, Identifier[] skinFrames, Texture cape, Identifier[] capeFrames) {
        this.skin = skin;
        this.skinFrames = skinFrames;
        this.cape = cape;
        this.capeFrames = capeFrames;
    }

    Identifier skinNow(long now) {
        return skinFrames == null ? null : skinFrames[skin.frameAt(now)];
    }

    Identifier capeNow(long now) {
        return capeFrames == null ? null : capeFrames[cape.frameAt(now)];
    }
}
