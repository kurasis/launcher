package app.launcher.gametest.agent;

import java.io.IOException;
import java.io.InputStream;
import java.lang.instrument.ClassFileTransformer;
import java.lang.instrument.Instrumentation;
import java.net.URL;
import java.security.ProtectionDomain;
import org.objectweb.asm.ClassReader;
import org.objectweb.asm.ClassVisitor;
import org.objectweb.asm.ClassWriter;
import org.objectweb.asm.MethodVisitor;
import org.objectweb.asm.Opcodes;
import org.objectweb.asm.tree.ClassNode;
import org.objectweb.asm.tree.InsnList;
import org.objectweb.asm.tree.MethodInsnNode;
import org.objectweb.asm.tree.MethodNode;
import org.objectweb.asm.tree.VarInsnNode;

/**
 * The test bot for a game with no mod loader (docs/GAMETEST.md): {@code -javaagent:} on
 * the game's command line. It rewrites {@code Minecraft.tick()} to call
 * {@link AgentHooks#tick} first, where the loaders' Mixin injects.
 *
 * <p>Before 26.1 the released game's names are obfuscated and the jar is remapped to them.
 * Remapping rewrites calls but not strings, so the target is read from the one call in
 * {@link AgentHooks#target}: its owner and name are the game's own.
 */
public final class Agent {
    private Agent() {}

    private static final String HOOKS = AgentHooks.class.getName().replace('.', '/');
    private static final String MESSAGE = "Launcher game test: ";

    /**
     * OptiFine on its own runs the game in LaunchWrapper's class loader, which defines the
     * game's classes itself from the class path; on Java 9 and later that path does not
     * hold this jar, so the calls the rewritten class makes would not resolve. That loader
     * is given this jar once, before the class is returned.
     */
    static void reach(ClassLoader loader) {
        if (loader == null || !"net.minecraft.launchwrapper.LaunchClassLoader".equals(loader.getClass().getName())
                || loader.getResource(HOOKS + ".class") != null) {
            return;
        }
        try {
            URL jar = Agent.class.getProtectionDomain().getCodeSource().getLocation();
            loader.getClass().getMethod("addURL", URL.class).invoke(loader, jar);
        } catch (ReflectiveOperationException | RuntimeException e) {
            System.err.println(MESSAGE + "LaunchWrapper did not take the agent (" + e.getClass().getSimpleName() + ")");
        }
    }

    public static void premain(String args, Instrumentation instrumentation) {
        MethodInsnNode target = target();
        if (target == null) {
            System.err.println("Launcher game test: the agent's target was not found; the game runs unchanged");
            return;
        }
        instrumentation.addTransformer(new Transformer(target));
    }

    /** The call inside {@code AgentHooks.target}, as the jar has it. */
    static MethodInsnNode target() {
        String own = AgentHooks.class.getName().replace('.', '/') + ".class";
        try (InputStream in = Agent.class.getClassLoader().getResourceAsStream(own)) {
            if (in == null) {
                return null;
            }
            MethodInsnNode[] found = new MethodInsnNode[1];
            new ClassReader(in).accept(new ClassVisitor(Opcodes.ASM9) {
                @Override
                public MethodVisitor visitMethod(int access, String name, String desc, String sig, String[] ex) {
                    if (!name.equals("target")) {
                        return null;
                    }
                    return new MethodVisitor(Opcodes.ASM9) {
                        @Override
                        public void visitMethodInsn(int op, String owner, String method, String type, boolean itf) {
                            if (op == Opcodes.INVOKEVIRTUAL && type.equals("()V")) {
                                found[0] = new MethodInsnNode(op, owner, method, type, itf);
                            }
                        }
                    };
                }
            }, ClassReader.SKIP_DEBUG | ClassReader.SKIP_FRAMES);
            return found[0];
        } catch (IOException e) {
            return null;
        }
    }

    static final class Transformer implements ClassFileTransformer {
        private final MethodInsnNode target;

        Transformer(MethodInsnNode target) {
            this.target = target;
        }

        @Override
        public byte[] transform(ClassLoader loader, String name, Class<?> redefined,
                ProtectionDomain domain, byte[] bytes) {
            if (!target.owner.equals(name)) {
                return null;
            }
            try {
                byte[] hooked = hook(bytes, target);
                if (hooked != null) {
                    reach(loader);
                }
                return hooked;
            } catch (RuntimeException e) {
                System.err.println("Launcher game test: Minecraft left unchanged (" + e.getClass().getSimpleName() + ")");
                return null;
            }
        }
    }

    /**
     * The class with the target method calling the hook first; null when the method is
     * missing. The call goes before the first instruction and adds no branch, so the
     * stack map frames stay valid.
     */
    static byte[] hook(byte[] bytes, MethodInsnNode target) {
        ClassNode node = new ClassNode();
        new ClassReader(bytes).accept(node, 0);
        for (MethodNode method : node.methods) {
            if ((method.access & Opcodes.ACC_STATIC) == 0 && method.name.equals(target.name)
                    && method.desc.equals(target.desc)) {
                InsnList call = new InsnList();
                call.add(new VarInsnNode(Opcodes.ALOAD, 0));
                call.add(new MethodInsnNode(Opcodes.INVOKESTATIC, AgentHooks.class.getName().replace('.', '/'),
                        "tick", "(L" + target.owner + ";)V", false));
                method.instructions.insert(call);
                method.maxStack = Math.max(method.maxStack, 1);
                ClassWriter writer = new ClassWriter(0);
                node.accept(writer);
                return writer.toByteArray();
            }
        }
        return null;
    }
}
