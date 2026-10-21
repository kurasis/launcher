package app.launcher.gametest.game;

import app.launcher.gametest.core.Game;
import java.util.Properties;
import net.minecraft.client.CameraType;
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
import net.minecraft.core.RegistryAccess;
import net.minecraft.resources.ResourceLocation;
import net.minecraft.world.Difficulty;
import net.minecraft.world.level.DataPackConfig;
import net.minecraft.world.level.GameRules;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.LevelSettings;
import net.minecraft.world.level.levelgen.WorldGenSettings;

/** The test bot's hands in Minecraft 1.17.1, shared by every loader. Plain Java 8. */
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
        LevelSettings settings = new LevelSettings(name, GameType.CREATIVE, false, Difficulty.NORMAL, true,
                new GameRules(), DataPackConfig.DEFAULT);
        RegistryAccess.RegistryHolder registries = RegistryAccess.builtin();
        // The way a dedicated server reads server.properties: a default world with this seed.
        Properties properties = new Properties();
        properties.setProperty("level-seed", Long.toString(seed));
        properties.setProperty("generator-settings", "");
        properties.setProperty("generate-structures", "true");
        properties.setProperty("level-type", "default");
        mc.createLevel(name, settings, registries, WorldGenSettings.create(registries, properties));
    }

    @Override
    public void unattended() {
        // 1.17.1 has no lower frame rate for an idle player.
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
        mc.options.keyUp.setDown(forward);
        mc.options.keyLeft.setDown(left);
        mc.options.keySprint.setDown(sprint);
        mc.options.keyJump.setDown(jump);
    }

    @Override
    public float yaw() {
        return mc.player == null ? 0 : mc.player.getYRot();
    }

    @Override
    public void look(float yaw, float pitch) {
        if (mc.player != null) {
            mc.player.setYRot(yaw);
            mc.player.setXRot(pitch);
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
        mc.options.setCameraType(CameraType.values()[Math.max(0, Math.min(2, view))]);
    }

    @Override
    public void flying(boolean flying) {
        LocalPlayer player = mc.player;
        if (player != null && player.getAbilities().mayfly && player.getAbilities().flying != flying) {
            player.getAbilities().flying = flying;
            player.onUpdateAbilities();
        }
    }

    @Override
    public boolean dead() {
        return mc.player != null && mc.player.isDeadOrDying();
    }

    @Override
    public void respawn() {
        if (mc.player != null) {
            mc.player.respawn();
        }
    }

    @Override
    public void screenshot(String fileName) {
        Screenshot.grab(mc.gameDirectory, fileName, mc.getMainRenderTarget(), message -> { });
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
