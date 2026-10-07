export interface SettingWriter<T> {
  write: (
    value: T,
    persist: (value: T) => Promise<unknown>,
    rollback: (persisted: T) => void,
  ) => void;
  persisted: (value: T) => void;
  idle: () => Promise<void>;
}

interface PendingWrite<T> {
  value: T;
  persist: (value: T) => Promise<unknown>;
  rollback: (persisted: T) => void;
}

export function createSettingWriter<T>(initial: T): SettingWriter<T> {
  let persisted = initial;
  let queued: PendingWrite<T> | null = null;
  let running: Promise<void> | null = null;

  const drain = async (first: PendingWrite<T>) => {
    let current: PendingWrite<T> | null = first;
    while (current) {
      queued = null;
      try {
        await current.persist(current.value);
        persisted = current.value;
      } catch {
        if (queued === null) current.rollback(persisted);
      }
      current = queued;
    }
    running = null;
  };

  return {
    persisted(value) {
      persisted = value;
    },
    async idle() {
      while (running) await running;
    },
    write(value, persist, rollback) {
      const next = { value, persist, rollback };
      queued = next;
      if (running) return;
      running = drain(next);
    },
  };
}
