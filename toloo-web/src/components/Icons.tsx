import type { Component, JSX } from "solid-js";

type IconProps = JSX.SvgSVGAttributes<SVGSVGElement> & { size?: number };

function IconBase(props: IconProps & { children: JSX.Element }): JSX.Element {
    const { size = 18, children, ...rest } = props;
    return (
        <svg
            viewBox="0 0 24 24"
            width={size}
            height={size}
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            {...rest}
        >
            {children}
        </svg>
    );
}

export const PlusIcon: Component<IconProps> = (props) => (
    <IconBase {...props}>
        <path d="M12 5v14M5 12h14" />
    </IconBase>
);

export const SettingsIcon: Component<IconProps> = (props) => (
    <IconBase {...props}>
        <circle cx="12" cy="12" r="3" />
        <path d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.83 2.83-.06-.06a1.7 1.7 0 0 0-1.88-.34 1.7 1.7 0 0 0-1.03 1.56V21h-4v-.09A1.7 1.7 0 0 0 8.94 19.4a1.7 1.7 0 0 0-1.88.34l-.06.06-2.83-2.83.06-.06A1.7 1.7 0 0 0 4.57 15 1.7 1.7 0 0 0 3 13.97H3v-4h.09A1.7 1.7 0 0 0 4.6 8.94a1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.83-2.83.06.06A1.7 1.7 0 0 0 9 4.57 1.7 1.7 0 0 0 10.03 3H10v4h-.03A1.7 1.7 0 0 0 9 8.94" />
    </IconBase>
);

export const SyncIcon: Component<IconProps> = (props) => (
    <IconBase {...props}>
        <path d="M20 11a8 8 0 0 0-14.9-3M4 5v6h6" />
        <path d="M4 13a8 8 0 0 0 14.9 3M20 19v-6h-6" />
    </IconBase>
);

export const SendIcon: Component<IconProps> = (props) => (
    <IconBase {...props}>
        <path d="m4 12 16-8-6.5 16-2.5-6.5L4 12Z" />
    </IconBase>
);

export const CopyIcon: Component<IconProps> = (props) => (
    <IconBase {...props}>
        <rect x="9" y="9" width="11" height="11" rx="2" />
        <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </IconBase>
);

export const CheckIcon: Component<IconProps> = (props) => (
    <IconBase {...props}>
        <path d="m5 12 4.5 4.5L19 7" />
    </IconBase>
);

export const ClockIcon: Component<IconProps> = (props) => (
    <IconBase {...props}>
        <circle cx="12" cy="12" r="9" />
        <path d="M12 7v5l3 2" />
    </IconBase>
);

export const ArrowLeftIcon: Component<IconProps> = (props) => (
    <IconBase {...props}>
        <path d="m15 19-7-7 7-7" />
    </IconBase>
);

export const PeopleIcon: Component<IconProps> = (props) => (
    <IconBase {...props}>
        <circle cx="9" cy="8" r="3.2" />
        <path d="M3.5 20a5.5 5.5 0 0 1 11 0M16 5.5a3 3 0 0 1 0 5.8M17 14.5a5 5 0 0 1 3.5 4.8" />
    </IconBase>
);

export const LockIcon: Component<IconProps> = (props) => (
    <IconBase {...props}>
        <rect x="4" y="10" width="16" height="11" rx="2.5" />
        <path d="M8 10V7a4 4 0 0 1 8 0v3" />
    </IconBase>
);

export const CloseIcon: Component<IconProps> = (props) => (
    <IconBase {...props}>
        <path d="M6 6l12 12M18 6 6 18" />
    </IconBase>
);

export const TrashIcon: Component<IconProps> = (props) => (
    <IconBase {...props}>
        <path d="M4 7h16M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2M6 7l1 13h10l1-13M10 11v5M14 11v5" />
    </IconBase>
);

export const ShieldIcon: Component<IconProps> = (props) => (
    <IconBase {...props}>
        <path d="M12 3 5 6v5c0 4.5 2.8 8 7 10 4.2-2 7-5.5 7-10V6l-7-3Z" />
        <path d="m9 12 2 2 4-4" />
    </IconBase>
);
