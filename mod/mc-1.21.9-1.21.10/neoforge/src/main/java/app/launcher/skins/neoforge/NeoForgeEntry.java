package app.launcher.skins.neoforge;

import app.launcher.skins.game.LauncherSkins;
import net.neoforged.api.distmarker.Dist;
import net.neoforged.fml.ModContainer;
import net.neoforged.fml.common.Mod;
import net.neoforged.fml.loading.FMLPaths;

/** NeoForge starts Launcher Skins here, on the client only. */
@Mod(value = LauncherSkins.ID, dist = Dist.CLIENT)
public final class NeoForgeEntry {
    public NeoForgeEntry(ModContainer mod) {
        LauncherSkins.start(FMLPaths.GAMEDIR.get(), mod.getModInfo().getVersion().toString());
    }
}
