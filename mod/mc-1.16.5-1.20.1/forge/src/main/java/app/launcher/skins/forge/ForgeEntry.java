package app.launcher.skins.forge;

import app.launcher.skins.game.LauncherSkins;
import net.minecraftforge.api.distmarker.Dist;
import net.minecraftforge.fml.ModLoadingContext;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.loading.FMLEnvironment;
import net.minecraftforge.fml.loading.FMLPaths;

/** Forge (1.14.4 to 1.20.1) starts Launcher Skins here; on a server it does nothing. */
@Mod(LauncherSkins.ID)
public final class ForgeEntry {
    public ForgeEntry() {
        if (FMLEnvironment.dist == Dist.CLIENT) {
            String version = ModLoadingContext.get().getActiveContainer().getModInfo().getVersion().toString();
            LauncherSkins.start(FMLPaths.GAMEDIR.get(), version);
        }
    }
}
