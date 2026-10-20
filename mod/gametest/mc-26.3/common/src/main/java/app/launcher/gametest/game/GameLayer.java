package app.launcher.gametest.game;

import app.launcher.gametest.core.Game;
import net.minecraft.client.CameraType;
import net.minecraft.client.InactivityFpsLimit;
import net.minecraft.client.Minecraft;
import net.minecraft.client.Screenshot;
import net.minecraft.client.gui.screens.Screen;
import net.minecraft.client.gui.screens.TitleScreen;
import net.minecraft.client.gui.screens.inventory.InventoryScreen;
import net.minecraft.client.multiplayer.ClientPacketListener;
import net.minecraft.client.multiplayer.PlayerInfo;
import net.minecraft.client.player.LocalPlayer;
import net.minecraft.client.server.IntegratedServer;
import net.minecraft.world.Difficulty;
import net.minecraft.world.entity.player.PlayerModelType;
import net.minecraft.world.entity.player.PlayerSkin;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.LevelSettings;
import net.minecraft.world.level.WorldDataConfiguration;
import net.minecraft.world.level.levelgen.WorldOptions;
import net.minecraft.world.level.levelgen.presets.WorldPresets;

/** The test bot's hands in Minecraft 26.3, shared by every loader. */
public final class GameLayer implements Game {
    private final Minecraft mc;

    public GameLayer(Minecraft mc) {
        this.mc = mc;
    }

    @Override
    public String screen() {
        Screen screen = mc.gui.screen();
        return screen == null ? null : screen.getClass().getName();
    }

    @Override
    public boolean loading() {
        return mc.gui.overlay() != null;
    }

    @Override
    public boolean onTitle() {
        return mc.gui.screen() instanceof TitleScreen;
    }

    @Override
    public boolean worldOpen() {
        return mc.level != null && mc.player != null;
    }

    @Override
    public void createWorld(String name, long seed) {
        LevelSettings settings = new LevelSettings(name, GameType.CREATIVE,
                new LevelSettings.DifficultySettings(Difficulty.NORMAL, false, false), true,
                WorldDataConfiguration.DEFAULT);
        mc.createWorldOpenFlows().createFreshLevel(name, settings, new WorldOptions(seed, true, false),
                WorldPresets::createNormalWorldDimensions, mc.gui.screen());
    }

    @Override
    public void unattended() {
        mc.options.pauseOnLostFocus = false;
        mc.options.inactivityFpsLimit().set(InactivityFpsLimit.MINIMIZED);
    }

    @Override
    public int fps() {
        return mc.getFps();
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
        return server == null ? -1 : server.getAverageTickTimeNanos() / 1_000_000.0;
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
        ClientPacketListener connection = mc.getConnection();
        if (connection != null) {
            connection.sendCommand(command);
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
        Screenshot.grab(mc.gameDirectory, fileName, mc.gameRenderer.mainRenderTarget(), 1, message -> { });
    }

    @Override
    public void openInventory() {
        if (mc.player != null) {
            mc.gui.setScreen(new InventoryScreen(mc.player));
        }
    }

    @Override
    public void closeScreen() {
        mc.gui.setScreen(null);
    }

    private PlayerSkin skin() {
        ClientPacketListener connection = mc.getConnection();
        if (connection == null || mc.player == null) {
            return null;
        }
        PlayerInfo info = connection.getPlayerInfo(mc.player.getUUID());
        return info == null ? null : info.getSkin();
    }

    @Override
    public String skinTexture() {
        PlayerSkin skin = skin();
        return skin == null || skin.body() == null ? null : skin.body().texturePath().toString();
    }

    @Override
    public String capeTexture() {
        PlayerSkin skin = skin();
        return skin == null || skin.cape() == null ? null : skin.cape().texturePath().toString();
    }

    @Override
    public Boolean slim() {
        PlayerSkin skin = skin();
        return skin == null ? null : skin.model() == PlayerModelType.SLIM;
    }

    @Override
    public void quit() {
        mc.stop();
    }
}
