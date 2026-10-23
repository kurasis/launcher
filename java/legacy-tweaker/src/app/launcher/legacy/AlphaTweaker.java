package app.launcher.legacy;

import java.awt.AWTEvent;
import java.awt.Frame;
import java.awt.Toolkit;
import java.awt.event.AWTEventListener;
import java.awt.event.WindowEvent;

import net.minecraft.launchwrapper.AlphaVanillaTweaker;
import net.minecraft.launchwrapper.LaunchClassLoader;

/**
 * Mojang's tweaker for the oldest games (Classic to Alpha 1.0), which says when the
 * player closes the game window.
 *
 * <p>Launchwrapper's frame for these games ends them with code 1 and without a word
 * when its window is closed, the code a failed start ends with too. This tweaker
 * prints {@link #CLOSED} first, so the launcher tells the two apart: the toolkit's
 * listeners hear a window event before the window's own. Anything unexpected leaves
 * Mojang's behaviour.
 */
public final class AlphaTweaker extends AlphaVanillaTweaker {
    /** The line the launcher looks for (src-tauri/src/launch/crash.rs). */
    static final String CLOSED = "Launcher: the game window was closed";

    @Override
    public void injectIntoClassLoader(LaunchClassLoader classLoader) {
        super.injectIntoClassLoader(classLoader);
        try {
            Toolkit.getDefaultToolkit().addAWTEventListener(new AWTEventListener() {
                @Override
                public void eventDispatched(AWTEvent event) {
                    if (event.getID() == WindowEvent.WINDOW_CLOSING && event.getSource() instanceof Frame) {
                        System.out.println(CLOSED);
                    }
                }
            }, AWTEvent.WINDOW_EVENT_MASK);
        } catch (RuntimeException | LinkageError e) {
            System.out.println("Launcher: the window's close not watched: " + e.getClass().getName());
        }
    }
}
