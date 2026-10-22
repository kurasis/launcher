package net.minecraft.client;

import java.io.File;

/** A stand-in game: its thread reads the game folder as early as the real one does. */
public class Minecraft {
    private static File workDir;

    public static void main(String[] args) throws Exception {
        Thread game = new Thread(new Runnable() {
            public void run() {
                System.out.println("game thread folder: " + workDir);
            }
        });
        game.start();
        game.join();
    }
}
