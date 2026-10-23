package net.minecraft.launchwrapper;

import java.io.File;
import java.util.List;

/** Compile-time stand-in for launchwrapper 1.5 and 1.6; the game's own jar is used at run time. */
public class AlphaVanillaTweaker implements ITweaker {
    @Override
    public void acceptOptions(List<String> args, File gameDir, File assetsDir, String profile) {
    }

    @Override
    public void injectIntoClassLoader(LaunchClassLoader classLoader) {
    }

    @Override
    public String getLaunchTarget() {
        return null;
    }

    @Override
    public String[] getLaunchArguments() {
        return null;
    }
}
