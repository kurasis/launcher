package app.launcher.gametest.mixin;

import app.launcher.gametest.core.Driver;
import app.launcher.gametest.game.GameLayer;
import net.minecraft.client.Minecraft;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Unique;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/** The bot acts at the start of every client tick. */
@Mixin(Minecraft.class)
abstract class MinecraftMixin {
    @Unique
    private GameLayer launchergametest$layer;

    @Inject(method = "tick", at = @At("HEAD"))
    private void launchergametest$tick(CallbackInfo ci) {
        if (launchergametest$layer == null) {
            launchergametest$layer = new GameLayer((Minecraft) (Object) this);
        }
        Driver.tick(launchergametest$layer);
    }
}
