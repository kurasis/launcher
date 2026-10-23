import { Button } from '../../components/Button';
import { strings } from '../../lib/strings';
import { bringBack } from './approvalStore';

/**
 * The metered-connection question of an operation that waits on it: a button that shows
 * it again once closed without an answer. It stays while the question is open (under it,
 * out of reach), so the focus comes back to it when the question is closed again.
 */
export function AsideApproval({
  operationId,
  size = 'small',
}: {
  operationId: string;
  size?: 'small' | 'normal';
}) {
  return (
    <Button
      size={size}
      onClick={() => {
        bringBack(operationId);
      }}
    >
      {strings.metered.showQuestion}
    </Button>
  );
}
