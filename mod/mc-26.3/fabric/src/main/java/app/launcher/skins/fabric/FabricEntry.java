package app.launcher.skins.fabric;

import app.launcher.skins.game.LauncherSkins;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.loader.api.FabricLoader;

/** Fabric (and Quilt, which runs Fabric mods) start Launcher Skins here. */
public final class FabricEntry implements ClientModInitializer {
    @Override
    public void onInitializeClient() {
        String version = FabricLoader.getInstance()
                .getModContainer(LauncherSkins.ID)
                .map(mod -> mod.getMetadata().getVersion().getFriendlyString())
                .orElse("unknown");
        LauncherSkins.start(FabricLoader.getInstance().getGameDir(), version);
    }
}
