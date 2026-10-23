package app.launcher.skins.game;

import com.mojang.blaze3d.platform.NativeImage;
import net.minecraft.client.Minecraft;
import net.minecraft.client.renderer.texture.TextureResources;
import net.minecraft.resources.Identifier;

/** Gives the game a texture made here: from 26.4 a texture is registered as its resources. */
final class Textures {
    private Textures() {}

    static void register(Identifier id, NativeImage image) {
        Minecraft.getInstance().getTextureManager().register(id, TextureResources.from2dImage(id::toString, image));
    }
}
