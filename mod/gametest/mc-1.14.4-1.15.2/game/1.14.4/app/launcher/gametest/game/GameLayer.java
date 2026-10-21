package app.launcher.gametest.game;

import app.launcher.gametest.core.Game;
import com.mojang.blaze3d.platform.InputConstants;
import net.minecraft.client.KeyMapping;
import net.minecraft.client.Minecraft;
import net.minecraft.client.Screenshot;
import net.minecraft.client.gui.screens.ChatScreen;
import net.minecraft.client.gui.screens.LevelLoadingScreen;
import net.minecraft.client.gui.screens.ProgressScreen;
import net.minecraft.client.gui.screens.ReceivingLevelScreen;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.client.gui.screens.TitleScreen;
import net.minecraft.client.gui.screens.inventory.InventoryScreen;
import net.minecraft.client.multiplayer.ClientPacketListener;
import net.minecraft.client.multiplayer.PlayerInfo;
import net.minecraft.client.player.LocalPlayer;
import net.minecraft.client.server.IntegratedServer;
import net.minecraft.resources.ResourceLocation;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.LevelSettings;
import net.minecraft.world.level.LevelType;

/** The test bot's hands in Minecraft 1.14.4, shared by every loader. Plain Java 8. */
public final class GameLayer implements Game {
    private final Minecraft mc;

    public GameLayer(Minecraft mc) {
        this.mc = mc;
    }

    @Override
    public String screen() {
        Screen screen = mc.screen;
        if (screen == null) {
            return null;
        }
        // Fabric runs this release with intermediary class names; the bot and the report
        // go by Mojang's names, so the screens it looks for are named here.
        if (screen instanceof TitleScreen) {
            return "TitleScreen";
        }
        if (screen instanceof ChatScreen) {
            return "ChatScreen";
        }
        if (screen instanceof LevelLoadingScreen) {
            return "LevelLoadingScreen";
        }
        if (screen instanceof ReceivingLevelScreen) {
            return "ReceivingLevelScreen";
        }
        if (screen instanceof ProgressScreen) {
            return "ProgressScreen";
        }
        return screen.getClass().getName() + " \"" + screen.getTitle().getString() + "\"";
    }

    @Override
    public boolean loading() {
        return mc.getOverlay() != null;
    }

    @Override
    public boolean onTitle() {
        return mc.screen instanceof TitleScreen;
    }

    @Override
    public boolean worldOpen() {
        return mc.level != null && mc.player != null;
    }

    @Override
    public void createWorld(String name, long seed) {
        // A default world with this seed, structures on, commands allowed.
        LevelSettings settings = new LevelSettings(seed, GameType.CREATIVE, true, false, LevelType.NORMAL)
                .enableSinglePlayerCommands();
        mc.selectLevel(name, name, settings);
    }

    @Override
    public void unattended() {
        // 1.14.4 has no lower frame rate for an idle player.
        mc.options.pauseOnLostFocus = false;
    }

    @Override
    public int fps() {
        // The frame counter itself is private; the debug screen's text starts with it.
        String text = mc.fpsString;
        int end = text == null ? -1 : text.indexOf(' ');
        try {
            return end > 0 ? Integer.parseInt(text.substring(0, end)) : 0;
        } catch (NumberFormatException e) {
            return 0;
        }
    }

    @Override
    public int chunks() {
        return mc.level == null ? 0 : mc.level.getChunkSource().getLoadedChunksCount();
    }

    @Override
    public int entities() {
        return mc.level == null ? 0 : mc.level.getEntityCount();
    }

    @Override
    public double tickMs() {
        IntegratedServer server = mc.getSingleplayerServer();
        return server == null ? -1 : server.getAverageTickTime();
    }

    @Override
    public void keys(boolean forward, boolean left, boolean sprint, boolean jump) {
        hold(mc.options.keyUp, forward);
        hold(mc.options.keyLeft, left);
        hold(mc.options.keySprint, sprint);
        hold(mc.options.keyJump, jump);
    }

    /** 1.14.4 has no setter for one key: press or release the key it is bound to. */
    private static void hold(KeyMapping mapping, boolean down) {
        KeyMapping.set(InputConstants.getKey(mapping.saveString()), down);
    }

    @Override
    public float yaw() {
        return mc.player == null ? 0 : mc.player.yRot;
    }

    @Override
    public void look(float yaw, float pitch) {
        if (mc.player != null) {
            mc.player.yRot = yaw;
            mc.player.xRot = pitch;
        }
    }

    @Override
    public void command(String command) {
        if (mc.player != null) {
            mc.player.chat("/" + command);
        }
    }

    @Override
    public void camera(int view) {
        mc.options.thirdPersonView = Math.max(0, Math.min(2, view));
    }

    @Override
    public void flying(boolean flying) {
        LocalPlayer player = mc.player;
        if (player != null && player.abilities.mayfly && player.abilities.flying != flying) {
            player.abilities.flying = flying;
            player.onUpdateAbilities();
        }
    }

    @Override
    public boolean dead() {
        return mc.player != null && mc.player.getHealth() <= 0;
    }

    @Override
    public void respawn() {
        if (mc.player != null) {
            mc.player.respawn();
        }
    }

    @Override
    public void screenshot(String fileName) {
        Screenshot.grab(mc.gameDirectory, fileName, mc.window.getWidth(), mc.window.getHeight(),
                mc.getMainRenderTarget(), message -> { });
    }

    @Override
    public void openInventory() {
        if (mc.player != null) {
            mc.setScreen(new InventoryScreen(mc.player));
        }
    }

    @Override
    public void closeScreen() {
        mc.setScreen(null);
    }

    private PlayerInfo info() {
        ClientPacketListener connection = mc.getConnection();
        return connection == null || mc.player == null ? null : connection.getPlayerInfo(mc.player.getUUID());
    }

    @Override
    public String skinTexture() {
        PlayerInfo info = info();
        ResourceLocation skin = info == null ? null : info.getSkinLocation();
        return skin == null ? null : skin.toString();
    }

    @Override
    public String capeTexture() {
        PlayerInfo info = info();
        ResourceLocation cape = info == null ? null : info.getCapeLocation();
        return cape == null ? null : cape.toString();
    }

    @Override
    public Boolean slim() {
        PlayerInfo info = info();
        return info == null ? null : "slim".equals(info.getModelName());
    }

    @Override
    public void quit() {
        mc.stop();
    }
}
