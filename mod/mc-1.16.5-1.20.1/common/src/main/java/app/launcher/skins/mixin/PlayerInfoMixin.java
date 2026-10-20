package app.launcher.skins.mixin;

import app.launcher.skins.game.LauncherSkins;
import net.minecraft.client.multiplayer.PlayerInfo;
import net.minecraft.resources.ResourceLocation;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/** Every place the game draws a player (body, cape, elytra, the player list) asks here. */
@Mixin(PlayerInfo.class)
abstract class PlayerInfoMixin {
    @Inject(method = "getSkinLocation", at = @At("RETURN"), cancellable = true)
    private void launcherskins$skin(CallbackInfoReturnable<ResourceLocation> cir) {
        cir.setReturnValue(LauncherSkins.skin(cir.getReturnValue(), self()));
    }

    @Inject(method = "getCapeLocation", at = @At("RETURN"), cancellable = true)
    private void launcherskins$cape(CallbackInfoReturnable<ResourceLocation> cir) {
        cir.setReturnValue(LauncherSkins.cape(cir.getReturnValue(), self()));
    }

    @Inject(method = "getElytraLocation", at = @At("RETURN"), cancellable = true)
    private void launcherskins$elytra(CallbackInfoReturnable<ResourceLocation> cir) {
        cir.setReturnValue(LauncherSkins.cape(cir.getReturnValue(), self()));
    }

    @Inject(method = "getModelName", at = @At("RETURN"), cancellable = true)
    private void launcherskins$model(CallbackInfoReturnable<String> cir) {
        cir.setReturnValue(LauncherSkins.model(cir.getReturnValue(), self()));
    }

    private PlayerInfo self() {
        return (PlayerInfo) (Object) this;
    }
}
