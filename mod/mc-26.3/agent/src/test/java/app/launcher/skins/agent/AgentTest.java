package app.launcher.skins.agent;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

import java.lang.classfile.ClassFile;
import java.lang.classfile.CodeElement;
import java.lang.classfile.MethodModel;
import java.lang.classfile.instruction.InvokeInstruction;
import java.lang.constant.ClassDesc;
import java.lang.constant.MethodTypeDesc;
import java.util.List;
import org.junit.jupiter.api.Test;

class AgentTest {
    private static final ClassDesc INFO = ClassDesc.ofInternalName(Agent.TARGET);
    private static final ClassDesc SKIN = ClassDesc.of("net.minecraft.world.entity.player.PlayerSkin");

    /** A stand-in PlayerInfo whose getSkin() returns from two places. */
    private static byte[] playerInfo() {
        return ClassFile.of().build(INFO, cls -> cls
                .withMethodBody("getSkin", MethodTypeDesc.of(SKIN), ClassFile.ACC_PUBLIC, code -> {
                    var other = code.newLabel();
                    code.iconst_0().ifeq(other).aconst_null().areturn().labelBinding(other).aconst_null().areturn();
                })
                .withMethodBody("getSkinLater", MethodTypeDesc.of(SKIN), ClassFile.ACC_PUBLIC,
                        code -> code.aconst_null().areturn()));
    }

    private static List<String> calls(byte[] bytes, String method) {
        MethodModel model = ClassFile.of().parse(bytes).methods().stream()
                .filter(m -> m.methodName().equalsString(method)).findFirst().orElseThrow();
        return model.code().orElseThrow().elementStream()
                .filter(e -> e instanceof InvokeInstruction)
                .map(e -> ((InvokeInstruction) e).owner().asInternalName() + "." + ((InvokeInstruction) e).name())
                .toList();
    }

    @Test
    void everyReturnOfGetSkinPassesThroughTheHook() {
        byte[] hooked = Agent.hook(playerInfo());
        String hook = AgentHooks.class.getName().replace('.', '/') + ".skin";
        assertEquals(List.of(hook, hook), calls(hooked, "getSkin"));
        assertEquals(List.of(), calls(hooked, "getSkinLater"));
        // The rewritten class still verifies as a class file.
        ClassFile.of().verify(hooked).forEach(error -> { throw error; });
    }

    @Test
    void junkIsRefused() {
        assertThrows(IllegalArgumentException.class, () -> Agent.hook(new byte[] {1, 2, 3}));
    }
}
