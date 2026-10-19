package app.launcher.skins.forge;

import app.launcher.skins.game.LauncherSkins;
import net.minecraftforge.api.distmarker.Dist;
import net.minecraftforge.fml.common.Mod;
import net.minecraftforge.fml.javafmlmod.FMLJavaModLoadingContext;
import net.minecraftforge.fml.loading.FMLEnvironment;
import net.minecraftforge.fml.loading.FMLPaths;

/** Forge starts Launcher Skins here; on a server it does nothing. */
@Mod(LauncherSkins.ID)
public final class ForgeEntry {
    public ForgeEntry(FMLJavaModLoadingContext context) {
        if (FMLEnvironment.dist == Dist.CLIENT) {
            String version = context.getContainer().getModInfo().getVersion().toString();
            LauncherSkins.start(FMLPaths.GAMEDIR.get(), version);
        }
    }
}
