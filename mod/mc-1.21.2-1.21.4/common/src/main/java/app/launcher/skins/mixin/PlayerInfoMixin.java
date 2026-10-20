package app.launcher.skins.mixin;

import app.launcher.skins.game.LauncherSkins;
import com.mojang.authlib.GameProfile;
import net.minecraft.client.multiplayer.PlayerInfo;
import net.minecraft.client.resources.PlayerSkin;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/** Every place the game draws a player (body, cape, elytra, the player list) asks here. */
@Mixin(PlayerInfo.class)
abstract class PlayerInfoMixin {
    @Shadow
    public abstract GameProfile getProfile();

    @Inject(method = "getSkin", at = @At("RETURN"), cancellable = true)
    private void launcherskins$skin(CallbackInfoReturnable<PlayerSkin> cir) {
        cir.setReturnValue(LauncherSkins.skin(getProfile(), cir.getReturnValue()));
    }
}
