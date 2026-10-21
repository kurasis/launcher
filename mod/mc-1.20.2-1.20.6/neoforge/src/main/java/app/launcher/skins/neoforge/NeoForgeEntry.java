package app.launcher.skins.neoforge;

import app.launcher.skins.game.LauncherSkins;
import net.neoforged.api.distmarker.Dist;
import net.neoforged.fml.ModLoadingContext;
import net.neoforged.fml.common.Mod;
import net.neoforged.fml.loading.FMLEnvironment;
import net.neoforged.fml.loading.FMLPaths;

/** NeoForge (1.20.2 to 1.20.4) starts Launcher Skins here; on a server it does nothing. */
@Mod(LauncherSkins.ID)
public final class NeoForgeEntry {
    public NeoForgeEntry() {
        if (FMLEnvironment.dist == Dist.CLIENT) {
            String version = ModLoadingContext.get().getActiveContainer().getModInfo().getVersion().toString();
            LauncherSkins.start(FMLPaths.GAMEDIR.get(), version);
        }
    }
}
