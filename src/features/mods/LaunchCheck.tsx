import { useEffect, useState } from 'react';
import { Button } from '../../components/Button';
import { Dialog } from '../../components/Dialog';
import dialog from '../../components/Dialog.module.css';
import { onLaunchQuestion, type LaunchQuestion } from '../../lib/launchCheck';
import { strings } from '../../lib/strings';
import { Problems } from './Problems';

/**
 * Asks before a launch when the mods' own files say the game may not start
 * (D-066): start anyway, look at the mods, or not now.
 */
export function LaunchCheck({ onOpenMods }: { onOpenMods: (instanceId: string) => void }) {
  const t = strings.mods.launchCheck;
  const [question, setQuestion] = useState<LaunchQuestion | null>(null);
  useEffect(() => onLaunchQuestion(setQuestion), []);
  if (question === null) return null;
  const decline = () => {
    question.answer(false);
  };
  return (
    <Dialog title={t.title} onClose={decline}>
      <Problems problems={question.problems} plain />
      <div className={dialog.actions}>
        <Button
          variant="primary"
          size="small"
          onClick={() => {
            question.answer(true);
          }}
        >
          {t.start}
        </Button>
        <Button
          size="small"
          onClick={() => {
            question.answer(false);
            onOpenMods(question.instanceId);
          }}
        >
          {t.openMods}
        </Button>
        <Button variant="ghost" size="small" onClick={decline}>
          {t.cancel}
        </Button>
      </div>
    </Dialog>
  );
}
