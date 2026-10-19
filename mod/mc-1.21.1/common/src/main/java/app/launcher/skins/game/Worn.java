package app.launcher.skins.game;

import app.launcher.skins.core.Texture;
import net.minecraft.resources.ResourceLocation;

/** A player's skin and cape as registered textures, one per frame. */
final class Worn {
    final Texture skin;
    final ResourceLocation[] skinFrames;
    final Texture cape;
    final ResourceLocation[] capeFrames;

    Worn(Texture skin, ResourceLocation[] skinFrames, Texture cape, ResourceLocation[] capeFrames) {
        this.skin = skin;
        this.skinFrames = skinFrames;
        this.cape = cape;
        this.capeFrames = capeFrames;
    }

    ResourceLocation skinNow(long now) {
        return skinFrames == null ? null : skinFrames[skin.frameAt(now)];
    }

    ResourceLocation capeNow(long now) {
        return capeFrames == null ? null : capeFrames[cape.frameAt(now)];
    }
}
