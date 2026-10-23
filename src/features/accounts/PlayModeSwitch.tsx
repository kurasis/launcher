import { Segmented } from '../../components/Segmented';
import { strings } from '../../lib/strings';

const a = strings.accounts;

/**
 * Demo or offline for players without a Minecraft token. Offline is the full
 * game without the account check, open only while a Microsoft account that
 * owns Minecraft is signed in here; the core checks the same at launch.
 */
export function PlayModeSwitch({
  offline,
  allowed,
  disabled,
  pending = false,
  onChange,
}: {
  offline: boolean;
  allowed: boolean;
  /** The settings cannot be written. */
  disabled: boolean;
  /** A change is being saved: the radios wait, but keep the focus. */
  pending?: boolean;
  onChange: (offline: boolean) => void;
}) {
  return (
    <Segmented
      label={a.playMode}
      options={[
        { value: false, label: a.modeDemo },
        { value: true, label: a.modeOffline, disabled: !allowed },
      ]}
      value={offline && allowed}
      disabled={disabled}
      pending={pending}
      onChange={onChange}
    />
  );
}
