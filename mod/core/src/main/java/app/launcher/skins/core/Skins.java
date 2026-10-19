package app.launcher.skins.core;

import java.io.IOException;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.RejectedExecutionException;
import java.util.concurrent.ThreadPoolExecutor;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicInteger;

/**
 * Who wears what. The game asks with {@link #want} whenever it draws a player (cheap enough
 * for every frame); profiles and textures load on two background threads and reach the game
 * through its {@link Sink}. A profile is asked again after 5 minutes, a nick that wears
 * nothing after a minute, a failure after 30 seconds.
 */
public final class Skins {
    /** Where loaded textures go: the game's side, which registers them on its own thread. */
    public interface Sink {
        /** This player's textures (PNG, checked against the profile; null if not worn). */
        void ready(String key, Profile profile, byte[] skin, byte[] cape);

        /** This player wears nothing of ours any more. */
        void gone(String key);
    }

    static final long FRESH_MS = 5 * 60_000L;
    static final long MISS_MS = 60_000L;
    static final long FAIL_MS = 30_000L;
    private static final int MAX_PLAYERS = 4096;

    private final Api api;
    private final DiskCache cache;
    private final Sink sink;
    private final ConcurrentHashMap<String, Entry> entries = new ConcurrentHashMap<>();
    private final ThreadPoolExecutor workers;

    private static final class Entry {
        final AtomicBoolean busy = new AtomicBoolean();
        volatile long next;
        volatile Profile profile;
    }

    public Skins(Api api, DiskCache cache, Sink sink) {
        this.api = api;
        this.cache = cache;
        this.sink = sink;
        AtomicInteger count = new AtomicInteger();
        this.workers = new ThreadPoolExecutor(2, 2, 30, TimeUnit.SECONDS, new ArrayBlockingQueue<Runnable>(256), r -> {
            Thread thread = new Thread(r, "Launcher Skins " + count.incrementAndGet());
            thread.setDaemon(true);
            return thread;
        });
        this.workers.allowCoreThreadTimeOut(true);
    }

    /** Asks for a player's skin and cape; does nothing while they are fresh or loading. */
    public void want(String nick) {
        if (!Nicks.valid(nick)) {
            return;
        }
        String key = Nicks.key(nick);
        Entry entry = entries.get(key);
        if (entry == null) {
            if (entries.size() >= MAX_PLAYERS) {
                entries.values().removeIf(e -> e.profile == null && !e.busy.get());
            }
            entry = entries.computeIfAbsent(key, k -> new Entry());
        }
        if (System.currentTimeMillis() < entry.next || !entry.busy.compareAndSet(false, true)) {
            return;
        }
        Entry asked = entry;
        try {
            workers.execute(() -> load(nick, key, asked));
        } catch (RejectedExecutionException e) {
            asked.busy.set(false);
        }
    }

    /** What this player wears now, or null. */
    public Profile profile(String nick) {
        Entry entry = Nicks.valid(nick) ? entries.get(Nicks.key(nick)) : null;
        return entry == null ? null : entry.profile;
    }

    /** Forgets everyone (leaving a world), so the next sight asks again. */
    public void clear() {
        entries.clear();
    }

    private void load(String nick, String key, Entry entry) {
        long now = System.currentTimeMillis();
        try {
            Profile profile = api.profile(nick);
            if (profile == null) {
                if (entry.profile != null) {
                    entry.profile = null;
                    sink.gone(key);
                }
                entry.next = now + MISS_MS;
                return;
            }
            if (!profile.sameAs(entry.profile)) {
                byte[] skin = profile.skin == null ? null : fetch(profile.skin);
                byte[] cape = profile.cape == null ? null : fetch(profile.cape);
                sink.ready(key, profile, skin, cape);
                entry.profile = profile;
            }
            entry.next = now + FRESH_MS;
        } catch (IOException | RuntimeException e) {
            entry.next = now + FAIL_MS;
        } finally {
            entry.busy.set(false);
        }
    }

    private byte[] fetch(Texture texture) throws IOException {
        byte[] png = cache.get(texture);
        if (png == null) {
            png = api.texture(texture);
            cache.put(texture, png);
        }
        return png;
    }
}
