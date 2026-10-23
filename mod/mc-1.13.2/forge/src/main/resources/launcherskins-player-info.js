// Forge 1.13.2 has no Mixin: this coremod passes what NetworkPlayerInfo returns
// for a player's skin, cape, elytra and model through LauncherSkins before every return,
// as the Mixin of the newer versions does. No branches are added, so the stack map frames
// stay valid. Methods are named by SRG name, which the game runs with.
var ASMAPI = Java.type('net.minecraftforge.coremod.api.ASMAPI');
var Opcodes = Java.type('org.objectweb.asm.Opcodes');
var InsnList = Java.type('org.objectweb.asm.tree.InsnList');
var VarInsnNode = Java.type('org.objectweb.asm.tree.VarInsnNode');
var MethodInsnNode = Java.type('org.objectweb.asm.tree.MethodInsnNode');

var INFO = 'net/minecraft/client/network/NetworkPlayerInfo';
var HOOKS = 'app/launcher/skins/game/LauncherSkins';
var LOCATION = 'Lnet/minecraft/util/ResourceLocation;';
var STRING = 'Ljava/lang/String;';

// SRG name → [the hook that filters its result, the type it returns].
var HOOKED = {
  func_178837_g: ['skin', LOCATION],
  func_178861_h: ['cape', LOCATION],
  func_187106_i: ['cape', LOCATION],
  func_178851_f: ['model', STRING],
};

function initializeCoreMod() {
  return {
    launcherskins_player_info: {
      target: { type: 'CLASS', name: 'net.minecraft.client.network.NetworkPlayerInfo' },
      transformer: function (node) {
        for (var srg in HOOKED) {
          var hook = HOOKED[srg][0];
          var type = HOOKED[srg][1];
          var name = ASMAPI.mapMethod(srg);
          for (var m = 0; m < node.methods.size(); m++) {
            var method = node.methods.get(m);
            if (method.name !== name || method.desc !== '()' + type) {
              continue;
            }
            var insns = method.instructions.toArray();
            for (var i = 0; i < insns.length; i++) {
              if (insns[i].getOpcode() === Opcodes.ARETURN) {
                var call = new InsnList();
                call.add(new VarInsnNode(Opcodes.ALOAD, 0));
                call.add(
                  new MethodInsnNode(
                    Opcodes.INVOKESTATIC,
                    HOOKS,
                    hook,
                    '(' + type + 'L' + INFO + ';)' + type,
                    false
                  )
                );
                method.instructions.insertBefore(insns[i], call);
              }
            }
            method.maxStack += 1;
          }
        }
        return node;
      },
    },
  };
}
