package app.launcher.skins.core;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.regex.Pattern;
import java.util.stream.Stream;

/**
 * Textures on disk, named by their SHA-256, so a skin seen once is not downloaded again.
 * A file is used only if its bytes still match its name.
 */
public final class DiskCache {
    private static final Pattern NAME = Pattern.compile("[0-9a-f]{64}\\.png");
    /** Files unused this long are removed at start. */
    private static final long KEEP_MS = 30L * 24 * 3600 * 1000;

    private final Path dir;

    public DiskCache(Path dir) {
        this.dir = dir;
    }

    /** The cached PNG of this texture, or null. */
    public byte[] get(Texture texture) {
        Path file = dir.resolve(texture.hash + ".png");
        try {
            if (!Files.isRegularFile(file) || Files.size(file) > texture.kind.maxBytes()) {
                return null;
            }
            byte[] png = Files.readAllBytes(file);
            if (!Api.matches(texture, png)) {
                Files.deleteIfExists(file);
                return null;
            }
            Files.setLastModifiedTime(file, java.nio.file.attribute.FileTime.fromMillis(System.currentTimeMillis()));
            return png;
        } catch (IOException e) {
            return null;
        }
    }

    /** Keeps a PNG that matched its texture. */
    public void put(Texture texture, byte[] png) {
        try {
            Files.createDirectories(dir);
            Path temp = Files.createTempFile(dir, "download", ".tmp");
            try {
                Files.write(temp, png);
                Files.move(temp, dir.resolve(texture.hash + ".png"), StandardCopyOption.REPLACE_EXISTING,
                        StandardCopyOption.ATOMIC_MOVE);
            } finally {
                Files.deleteIfExists(temp);
            }
        } catch (IOException e) {
            // A cache that cannot be written only means downloading again.
        }
    }

    /** Removes textures unused for 30 days and anything that is not a texture of ours. */
    public void tidy() {
        if (!Files.isDirectory(dir)) {
            return;
        }
        long now = System.currentTimeMillis();
        try (Stream<Path> files = Files.list(dir)) {
            files.forEach(file -> {
                try {
                    boolean ours = NAME.matcher(file.getFileName().toString()).matches();
                    if (Files.isRegularFile(file)
                            && (!ours || now - Files.getLastModifiedTime(file).toMillis() > KEEP_MS)) {
                        Files.deleteIfExists(file);
                    }
                } catch (IOException e) {
                    // Left for the next start.
                }
            });
        } catch (IOException e) {
            // Nothing to tidy.
        }
    }
}
