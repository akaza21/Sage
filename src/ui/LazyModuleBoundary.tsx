import {
  Component,
  Suspense,
  useEffect,
  useState,
  type ComponentType,
  type ReactNode,
} from "react";

type Module<Props extends object> = { default: ComponentType<Props> };
type ModuleLoader = () => Promise<Module<object>>;
const lazyModules = new WeakMap<ModuleLoader, ComponentType<object>>();
const pendingModules = new WeakMap<ModuleLoader, Promise<Module<object>>>();

function loadOnce(loader: ModuleLoader) {
  const pending = pendingModules.get(loader);
  if (pending) return pending;
  const request = Promise.resolve()
    .then(loader)
    .then(
      (module) => {
        lazyModules.set(loader, module.default);
        pendingModules.delete(loader);
        return module;
      },
      (error) => {
        pendingModules.delete(loader);
        throw error;
      },
    );
  pendingModules.set(loader, request);
  return request;
}

class ModuleLoadErrorBoundary extends Component<
  {
    children: ReactNode;
    label: string;
    onRetry: () => void;
  },
  { error: boolean }
> {
  state = { error: false };

  static getDerivedStateFromError() {
    return { error: true };
  }

  render() {
    if (this.state.error) {
      return (
        <section role="alert" aria-label={`${this.props.label} failed to load`}>
          <p>{this.props.label} could not be loaded.</p>
          <button type="button" onClick={this.props.onRetry}>
            Retry
          </button>
        </section>
      );
    }
    return this.props.children;
  }
}

type ModuleResult = { loader: ModuleLoader } & (
  | { phase: "loading" }
  | { phase: "ready"; Surface: ComponentType<object> }
  | { phase: "error"; error: unknown }
);

/** Resolve feature modules directly, avoiding nested Suspense retry throttling. */
export function DeferredModuleSurface<Props extends object>({
  load,
  render,
  fallback,
  attempt = 0,
}: {
  load: () => Promise<Module<Props>>;
  render: (Surface: ComponentType<Props>) => ReactNode;
  fallback: ReactNode;
  attempt?: number;
}) {
  const loader = load as unknown as ModuleLoader;
  const [result, setResult] = useState<ModuleResult>(() => {
    const Surface = attempt === 0 ? lazyModules.get(loader) : undefined;
    if (Surface) return { loader, phase: "ready", Surface };
    // Start the import during the first render; the effect observes its result.
    void loadOnce(loader).catch(() => undefined);
    return { loader, phase: "loading" };
  });
  useEffect(() => {
    if (result.loader === loader && result.phase !== "loading") return;
    const cached = attempt === 0 ? lazyModules.get(loader) : undefined;
    let active = true;
    const request = cached
      ? Promise.resolve({ default: cached })
      : loadOnce(loader);
    void request.then(
      (module) => {
        if (active)
          setResult({ loader, phase: "ready", Surface: module.default });
      },
      (error) => {
        if (active) setResult({ loader, phase: "error", error });
      },
    );
    return () => {
      active = false;
    };
  }, [loader, attempt, result]);
  if (result.loader === loader && result.phase === "error") throw result.error;
  const Surface =
    result.loader === loader && result.phase === "ready"
      ? result.Surface
      : attempt === 0
        ? lazyModules.get(loader)
        : undefined;
  return Surface ? render(Surface as ComponentType<Props>) : fallback;
}

/** A retry remounts the failed feature and invokes a fresh module request. */
export function LazyModuleBoundary<Props extends object>({
  load,
  label,
  render,
  fallback = <p role="status">Loading {label}…</p>,
}: {
  load: () => Promise<Module<Props>>;
  label: string;
  render: (Surface: ComponentType<Props>) => ReactNode;
  fallback?: ReactNode;
}) {
  const [attempt, setAttempt] = useState(0);
  return (
    <ModuleLoadErrorBoundary
      key={attempt}
      label={label}
      onRetry={() => setAttempt((value) => value + 1)}
    >
      <Suspense fallback={fallback}>
        <DeferredModuleSurface
          key={attempt}
          load={load}
          render={render}
          fallback={fallback}
          attempt={attempt}
        />
      </Suspense>
    </ModuleLoadErrorBoundary>
  );
}
