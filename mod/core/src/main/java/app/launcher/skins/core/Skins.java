package app.launcher.skins.core;

import java.io.IOException;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ConcurrentLinkedQueue;
import java.util.concurrent.RejectedExecutionException;
import java.util.concurrent.ThreadPoolExecutor;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicInteger;

/**
 * Who wears what. The game asks with {@link #want} whenever it draws a player (cheap enough
 * for every frame); profiles and textures load on four background threads and reach the game
 * through its {@link Sink}. Profiles are asked in batches of up to {@link Api#MAX_NICKS}, so
 * a hundred players cost two requests; their textures then load side by side. A profile is
 * asked again after 5 minutes, a nick that wears nothing after a minute, a failure after 30
 * seconds.
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
    private static final int THREADS = 4;

    private final Api api;
    private final DiskCache cache;
    private final Sink sink;
    private final ConcurrentHashMap<String, Entry> entries = new ConcurrentHashMap<>();
    /** Players whose profile is due, waiting for the next batch. */
    private final ConcurrentLinkedQueue<Wanted> due = new ConcurrentLinkedQueue<>();
    /** Whether a batch is being asked: one at a time. */
    private final AtomicBoolean asking = new AtomicBoolean();
    private final ThreadPoolExecutor workers;

    private static final class Entry {
        final AtomicBoolean busy = new AtomicBoolean();
        volatile long next;
        volatile Profile profile;
    }

    private static final class Wanted {
        final String nick;
        final String key;
        final Entry entry;

        Wanted(String nick, String key, Entry entry) {
            this.nick = nick;
            this.key = key;
            this.entry = entry;
        }
    }

    public Skins(Api api, DiskCache cache, Sink sink) {
        this.api = api;
        this.cache = cache;
        this.sink = sink;
        AtomicInteger count = new AtomicInteger();
        this.workers = new ThreadPoolExecutor(THREADS, THREADS, 30, TimeUnit.SECONDS,
                new ArrayBlockingQueue<Runnable>(256), r -> {
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
        if (System.currentTimeMillis() >= entry.next && entry.busy.compareAndSet(false, true)) {
            due.add(new Wanted(nick, key, entry));
        }
        ask();
    }

    /** What this player wears now, or null. */
    public Profile profile(String nick) {
        Entry entry = Nicks.valid(nick) ? entries.get(Nicks.key(nick)) : null;
        return entry == null ? null : entry.profile;
    }

    /** Forgets everyone (leaving a world), so the next sight asks again. */
    public void clear() {
        entries.clear();
        due.clear();
    }

    /** Starts a batch if players are due and none is being asked; a full pool waits for the next call. */
    private void ask() {
        if (due.isEmpty() || !asking.compareAndSet(false, true)) {
            return;
        }
        try {
            workers.execute(this::batches);
        } catch (RejectedExecutionException e) {
            asking.set(false);
        }
    }

    private void batches() {
        try {
            List<Wanted> batch = new ArrayList<>(Api.MAX_NICKS);
            for (Wanted wanted; batch.size() < Api.MAX_NICKS && (wanted = due.poll()) != null; ) {
                batch.add(wanted);
            }
            if (!batch.isEmpty()) {
                batch(batch);
            }
        } finally {
            asking.set(false);
        }
        // Players that came while this batch was asked go in the next one.
        ask();
    }

    private void batch(List<Wanted> batch) {
        long now = System.currentTimeMillis();
        List<String> nicks = new ArrayList<>(batch.size());
        for (Wanted wanted : batch) {
            nicks.add(wanted.nick);
        }
        Map<String, Profile> found;
        try {
            found = api.profiles(nicks);
        } catch (IOException | RuntimeException e) {
            for (Wanted wanted : batch) {
                done(wanted.entry, now + FAIL_MS);
            }
            return;
        }
        for (Wanted wanted : batch) {
            Profile profile = found.get(wanted.key);
            Entry entry = wanted.entry;
            if (profile == null) {
                if (entry.profile != null) {
                    entry.profile = null;
                    sink.gone(wanted.key);
                }
                done(entry, now + MISS_MS);
            } else if (profile.sameAs(entry.profile)) {
                done(entry, now + FRESH_MS);
            } else {
                Runnable load = () -> wear(wanted.key, entry, profile);
                try {
                    workers.execute(load);
                } catch (RejectedExecutionException e) {
                    load.run(); // A full pool: this thread loads it itself.
                }
            }
        }
    }

    private void wear(String key, Entry entry, Profile profile) {
        long now = System.currentTimeMillis();
        try {
            byte[] skin = profile.skin == null ? null : fetch(profile.skin);
            byte[] cape = profile.cape == null ? null : fetch(profile.cape);
            sink.ready(key, profile, skin, cape);
            entry.profile = profile;
            done(entry, now + FRESH_MS);
        } catch (IOException | RuntimeException e) {
            done(entry, now + FAIL_MS);
        }
    }

    private static void done(Entry entry, long next) {
        entry.next = next;
        entry.busy.set(false);
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
