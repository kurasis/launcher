package app.launcher.legacy;

import java.io.File;
import java.lang.reflect.Field;
import java.lang.reflect.Modifier;

import net.minecraft.launchwrapper.LaunchClassLoader;
import net.minecraft.launchwrapper.VanillaTweaker;

/**
 * Mojang's tweaker for Minecraft before 1.6 (launchwrapper 1.5), with the game
 * folder set before the game starts.
 *
 * <p>Launchwrapper 1.5 stores the game folder in the game's static File field
 * only at the end of {@code Minecraft.main}, after the game thread has started.
 * That thread reads the field once while starting; when it gets there first, it
 * takes the default {@code .minecraft} folder of the system user for the options,
 * saves and statistics. This tweaker sets the same field as soon as the class is
 * loaded, before {@code main} runs. Anything unexpected leaves Mojang's behaviour.
 */
public final class LegacyTweaker extends VanillaTweaker {
    @Override
    public void injectIntoClassLoader(LaunchClassLoader classLoader) {
        super.injectIntoClassLoader(classLoader);
        File gameDir = VanillaTweaker.gameDir;
        if (gameDir == null) {
            return;
        }
        try {
            Class<?> minecraft = Class.forName("net.minecraft.client.Minecraft", true, classLoader);
            for (Field field : minecraft.getDeclaredFields()) {
                if (Modifier.isStatic(field.getModifiers()) && field.getType() == File.class) {
                    field.setAccessible(true);
                    field.set(null, gameDir);
                    System.out.println("Launcher: game folder set before the start");
                    return;
                }
            }
        } catch (ReflectiveOperationException | LinkageError | RuntimeException e) {
            System.out.println("Launcher: game folder not set early: " + e.getClass().getName());
        }
    }
}
