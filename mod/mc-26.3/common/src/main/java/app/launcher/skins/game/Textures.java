package app.launcher.skins.game;

import com.mojang.blaze3d.platform.NativeImage;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.texture.DynamicTexture;
import net.minecraft.resources.Identifier;

/** Gives the game a texture made here; the newest snapshot has its own (mc-snapshot/common). */
final class Textures {
    private Textures() {}

    static void register(Identifier id, NativeImage image) {
        Minecraft.getInstance().getTextureManager().register(id, new DynamicTexture(id::toString, image));
    }
}
