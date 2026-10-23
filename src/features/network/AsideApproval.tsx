import { Button } from '../../components/Button';
import { strings } from '../../lib/strings';
import { bringBack, useAside } from './approvalStore';

/** The metered-connection question, closed without an answer: a button to see it again. */
export function AsideApproval({ operationId }: { operationId: string }) {
  const aside = useAside(operationId);
  if (!aside) return null;
  return (
    <Button
      size="small"
      onClick={() => {
        bringBack(operationId);
      }}
    >
      {strings.metered.showQuestion}
    </Button>
  );
}
