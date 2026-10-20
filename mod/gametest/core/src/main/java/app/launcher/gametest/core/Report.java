package app.launcher.gametest.core;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * What the bot saw, written as `report.json` for the test harness. It holds numbers,
 * screen class names and fixed words only: nothing about the computer or the player.
 */
public final class Report {
    /** One named check with its result. */
    public static final class Check {
        public final String name;
        public final boolean ok;
        public final String detail;

        Check(String name, boolean ok, String detail) {
            this.name = name;
            this.ok = ok;
            this.detail = detail;
        }
    }

    /** One part of the play and what the game did during it. */
    public static final class Phase {
        public final String name;
        public final Stats fps = new Stats();
        public final Stats chunks = new Stats();
        public final Stats entities = new Stats();
        public final Stats tickMs = new Stats();

        Phase(String name) {
            this.name = name;
        }
    }

    public final List<String> screensBeforeWorld = new ArrayList<>();
    public final List<String> screenshots = new ArrayList<>();
    public final List<String> problems = new ArrayList<>();
    public final List<Check> checks = new ArrayList<>();
    public final List<Phase> phases = new ArrayList<>();
    public final Stats fps = new Stats();
    public final Stats memoryMb = new Stats();
    public long titleAfterMs = -1;
    public long worldAfterMs = -1;
    public long worldLoadMs = -1;
    public long playedMs;
    public long skinAfterMs = -1;
    public int skinFramesSeen;
    public int capeFramesSeen;
    public int deaths;
    public boolean finished;

    void problem(String text) {
        if (problems.size() < 100 && !problems.contains(text)) {
            problems.add(text);
        }
    }

    void check(String name, boolean ok, String detail) {
        checks.add(new Check(name, ok, detail));
    }

    public boolean passed() {
        for (Check check : checks) {
            if (!check.ok) {
                return false;
            }
        }
        return finished;
    }

    /** Writes the report next to where it should end up and moves it into place. */
    public void write(File dir) throws IOException {
        if (!dir.isDirectory() && !dir.mkdirs()) {
            throw new IOException("no report folder");
        }
        File tmp = new File(dir, "report.json.tmp");
        File target = new File(dir, "report.json");
        try (OutputStream out = new FileOutputStream(tmp)) {
            out.write(toJson().getBytes(StandardCharsets.UTF_8));
        }
        if (target.exists() && !target.delete()) {
            throw new IOException("old report in the way");
        }
        if (!tmp.renameTo(target)) {
            throw new IOException("report not moved into place");
        }
    }

    public String toJson() {
        Map<String, Object> root = new LinkedHashMap<>();
        root.put("finished", finished);
        root.put("passed", passed());
        root.put("titleAfterMs", titleAfterMs);
        root.put("worldAfterMs", worldAfterMs);
        root.put("worldLoadMs", worldLoadMs);
        root.put("playedMs", playedMs);
        root.put("skinAfterMs", skinAfterMs);
        root.put("skinFramesSeen", skinFramesSeen);
        root.put("capeFramesSeen", capeFramesSeen);
        root.put("deaths", deaths);
        root.put("fps", stats(fps));
        root.put("memoryMb", stats(memoryMb));
        List<Object> checkList = new ArrayList<>();
        for (Check check : checks) {
            Map<String, Object> c = new LinkedHashMap<>();
            c.put("name", check.name);
            c.put("ok", check.ok);
            c.put("detail", check.detail);
            checkList.add(c);
        }
        root.put("checks", checkList);
        List<Object> phaseList = new ArrayList<>();
        for (Phase phase : phases) {
            Map<String, Object> p = new LinkedHashMap<>();
            p.put("name", phase.name);
            p.put("fps", stats(phase.fps));
            p.put("chunks", stats(phase.chunks));
            p.put("entities", stats(phase.entities));
            p.put("tickMs", stats(phase.tickMs));
            phaseList.add(p);
        }
        root.put("phases", phaseList);
        root.put("screensBeforeWorld", new ArrayList<Object>(screensBeforeWorld));
        root.put("screenshots", new ArrayList<Object>(screenshots));
        root.put("problems", new ArrayList<Object>(problems));
        StringBuilder out = new StringBuilder();
        Json.write(out, root);
        return out.append('\n').toString();
    }

    private static Map<String, Object> stats(Stats stats) {
        Map<String, Object> s = new LinkedHashMap<>();
        s.put("samples", stats.count());
        s.put("min", stats.min());
        s.put("avg", stats.avg());
        s.put("p5", stats.percentile(0.05));
        s.put("max", stats.max());
        return s;
    }
}
