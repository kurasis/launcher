package app.launcher.skins.core;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.sun.net.httpserver.HttpServer;
import java.io.IOException;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class ApiTest {
    /** Path → status and body the fake service answers. */
    private final Map<String, Object[]> answers = new ConcurrentHashMap<>();
    private HttpServer server;
    private Api api;

    private final byte[] skin = PngTest.png(64, 64 * 2);
    private final String skinHash = Api.sha256(skin);
    private final byte[] cape = PngTest.png(64, 32);
    private final String capeHash = Api.sha256(cape);

    @BeforeEach
    void start() throws IOException {
        server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext("/", exchange -> {
            Object[] answer = answers.getOrDefault(exchange.getRequestURI().getPath(), new Object[] {404, new byte[0]});
            byte[] body = (byte[]) answer[1];
            if (answer.length > 2) {
                exchange.getResponseHeaders().add("Location", (String) answer[2]);
            }
            exchange.sendResponseHeaders((Integer) answer[0], body.length == 0 ? -1 : body.length);
            try (OutputStream out = exchange.getResponseBody()) {
                out.write(body);
            }
        });
        server.start();
        api = new Api(Api.check("http://127.0.0.1:" + server.getAddress().getPort()), "test");
    }

    @AfterEach
    void stop() {
        server.stop(0);
    }

    private void answer(String path, int status, String json) {
        answers.put(path, new Object[] {status, json.getBytes(StandardCharsets.UTF_8)});
    }

    private String profileJson(String nick) {
        return "{\"nick\":\"" + nick + "\",\"skin\":{\"hash\":\"" + skinHash + "\",\"model\":\"slim\",\"width\":64,"
                + "\"height\":128,\"frames\":2,\"frame_ms\":[100,200],\"url\":\"https://evil.example/x\"},"
                + "\"cape\":{\"hash\":\"" + capeHash + "\",\"width\":64,\"height\":32,\"frames\":1,\"frame_ms\":[]}}";
    }

    @Test
    void addresses() {
        assertEquals("https://skins.lapi.sh", Api.check("https://skins.lapi.sh"));
        assertEquals("https://skins.lapi.sh", Api.check(" https://skins.lapi.sh/ "));
        assertEquals("https://example.com:8443", Api.check("https://example.com:8443"));
        assertEquals("http://127.0.0.1:9", Api.check("http://127.0.0.1:9"));
        for (String bad : new String[] {"http://skins.lapi.sh", "ftp://skins.lapi.sh", "file:///etc/passwd",
            "https://user:pw@skins.lapi.sh", "https://skins.lapi.sh/x", "https://skins.lapi.sh?a=1",
            "https://skins.lapi.sh#x", "javascript:alert(1)", "not a url", "https://"}) {
            assertNull(Api.check(bad), bad);
        }
        System.setProperty(Api.PROPERTY, "http://evil.example");
        try {
            assertNotNull(Api.fromSystem("t"));
        } finally {
            System.clearProperty(Api.PROPERTY);
        }
    }

    @Test
    void readsAProfile() throws IOException {
        answer("/v1/profile/Steve", 200, profileJson("steve"));
        answers.put("/v1/textures/" + skinHash, new Object[] {200, skin});
        Profile profile = api.profile("Steve");
        assertNotNull(profile);
        assertTrue(profile.skin.slim);
        assertEquals(2, profile.skin.frames);
        assertArrayEquals(new int[] {100, 200}, profile.skin.frameMs());
        assertEquals(64, profile.skin.frameHeight());
        assertEquals(1, profile.cape.frames);
        assertArrayEquals(skin, api.texture(profile.skin));
    }

    @Test
    void refusesWhatBreaksTheRules() throws IOException {
        answer("/v1/profile/Steve", 200, profileJson("Alex"));
        assertNull(api.profile("Steve"), "another nick");
        answer("/v1/profile/Steve", 200, profileJson("Steve").replace("\"frames\":2", "\"frames\":3"));
        Profile capeOnly = api.profile("Steve");
        assertNull(capeOnly.skin, "a broken skin is left out");
        assertNotNull(capeOnly.cape);
        answer("/v1/profile/Steve", 200, profileJson("Steve").replace(skinHash, "../../etc/passwd"));
        assertNull(api.profile("Steve").skin);
        answer("/v1/profile/Steve", 200, profileJson("Steve").replace("\"slim\"", "\"giant\""));
        assertNull(api.profile("Steve").skin);
        answer("/v1/profile/Steve", 200, profileJson("Steve").replace("[100,200]", "[100,1]"));
        assertNull(api.profile("Steve").skin);
        answer("/v1/profile/Steve", 200, "[1,2,3]");
        assertNull(api.profile("Steve"));
        answer("/v1/profile/Steve", 200, "{not json");
        assertThrows(IOException.class, () -> api.profile("Steve"));
        assertNull(api.profile("../v1/textures"), "a bad nick is never asked");
        answers.remove("/v1/profile/Steve");
        assertNull(api.profile("Steve"), "404 is nothing worn");
        answer("/v1/profile/Steve", 500, "");
        assertThrows(IOException.class, () -> api.profile("Steve"));
        answers.put("/v1/profile/Steve", new Object[] {302, new byte[0], "https://evil.example/"});
        assertThrows(IOException.class, () -> api.profile("Steve"), "no redirects");
        byte[] huge = new byte[Api.MAX_JSON + 1];
        Arrays.fill(huge, (byte) ' ');
        answers.put("/v1/profile/Steve", new Object[] {200, huge});
        assertThrows(IOException.class, () -> api.profile("Steve"), "size limit");
    }

    @Test
    void refusesATextureThatIsNotTheOneDescribed() throws IOException {
        answer("/v1/profile/Steve", 200, profileJson("Steve"));
        Profile profile = api.profile("Steve");
        answers.put("/v1/textures/" + skinHash, new Object[] {200, cape});
        assertThrows(IOException.class, () -> api.texture(profile.skin));
    }

    @Test
    void readsSeveralProfiles() throws IOException {
        answer("/v1/profiles", 200, "{\"profiles\":{\"Steve\":" + profileJson("Steve") + ",\"Alex\":null}}");
        Map<String, Profile> found = api.profiles(Arrays.asList("Steve", "Alex", "bad nick"));
        assertEquals(1, found.size());
        assertNotNull(found.get("steve"));
    }

    @Test
    void loadsSkinsInTheBackgroundAndCachesThem(@TempDir Path dir) throws Exception {
        answer("/v1/profile/Steve", 200, profileJson("Steve"));
        answers.put("/v1/textures/" + skinHash, new Object[] {200, skin});
        answers.put("/v1/textures/" + capeHash, new Object[] {200, cape});
        CountDownLatch ready = new CountDownLatch(1);
        AtomicReference<byte[]> got = new AtomicReference<>();
        Skins skins = new Skins(api, new DiskCache(dir), new Skins.Sink() {
            @Override
            public void ready(String key, Profile profile, byte[] skinPng, byte[] capePng) {
                got.set(skinPng);
                ready.countDown();
            }

            @Override
            public void gone(String key) {}
        });
        skins.want("Steve");
        skins.want("Steve");
        assertTrue(ready.await(10, TimeUnit.SECONDS));
        assertArrayEquals(skin, got.get());
        assertTrue(Files.isRegularFile(dir.resolve(skinHash + ".png")));

        DiskCache cache = new DiskCache(dir);
        Profile profile = skins.profile("STEVE");
        assertArrayEquals(skin, cache.get(profile.skin));
        Files.write(dir.resolve(skinHash + ".png"), cape);
        assertNull(cache.get(profile.skin), "a changed file is not used");
        Files.write(dir.resolve("other.txt"), new byte[] {1});
        cache.tidy();
        assertTrue(Files.notExists(dir.resolve("other.txt")));
    }
}
