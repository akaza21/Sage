import {
  Component,
  lazy,
  Suspense,
  useMemo,
  useState,
  type ComponentType,
  type ReactNode,
} from "react";

type Module<Props extends object> = { default: ComponentType<Props> };
type ModuleLoader = () => Promise<Module<object>>;
const lazyModules = new WeakMap<ModuleLoader, ComponentType<object>>();

class ModuleLoadErrorBoundary extends Component<
  {
    children: ReactNode;
    label: string;
    onRetry: () => void;
    resetKey: number;
  },
  { error: unknown }
> {
  state = { error: null };

  static getDerivedStateFromError(error: unknown) {
    return { error };
  }

  componentDidUpdate(previousProps: Readonly<{ resetKey: number }>) {
    if (previousProps.resetKey !== this.props.resetKey && this.state.error) {
      this.setState({ error: null });
    }
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

/** A retry creates a new React.lazy loader, rather than re-rendering a rejected one. */
export function LazyModuleBoundary<Props extends object>({
  load,
  label,
  render,
}: {
  load: () => Promise<Module<Props>>;
  label: string;
  render: (Surface: ComponentType<Props>) => ReactNode;
}) {
  const [attempt, setAttempt] = useState(0);
  const Surface = useMemo(() => {
    const loader = load as unknown as ModuleLoader;
    if (attempt === 0) {
      const cached = lazyModules.get(loader);
      if (cached) return cached as ComponentType<Props>;
    }
    const next = lazy(() =>
      load().then((module) => {
        lazyModules.set(loader, module.default as ComponentType<object>);
        return module;
      }),
    );
    return next;
  }, [attempt, load]);
  const retry = () => setAttempt((value) => value + 1);
  const content = render(Surface);

  return (
    <ModuleLoadErrorBoundary label={label} onRetry={retry} resetKey={attempt}>
      <Suspense fallback={<p role="status">Loading {label}…</p>}>
        {content}
      </Suspense>
    </ModuleLoadErrorBoundary>
  );
}
