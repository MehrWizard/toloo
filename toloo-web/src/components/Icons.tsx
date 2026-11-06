import type { Component, JSX } from "solid-js";
import { ICONS, type IconName } from "../generated/icons";

type IconProps = JSX.SvgSVGAttributes<SVGSVGElement> & { size?: number };

function icon(name: IconName): Component<IconProps> {
    const inner = ICONS[name]
        .replace(/^<svg[^>]*>/, "")
        .replace(/<\/svg>$/, "");

    return (props) => {
        const { size = 18, ...rest } = props;
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
                innerHTML={inner}
            />
        );
    };
}

export const ArrowLeftIcon = icon("back");
export const BlockIcon = icon("block");
export const CheckIcon = icon("check");
export const ClockIcon = icon("clock");
export const CloseIcon = icon("close");
export const CopyIcon = icon("copy");
export const EditIcon = icon("edit");
export const ExportIcon = icon("export");
export const FlagIcon = icon("flag");
export const InboxIcon = icon("inbox");
export const LockIcon = icon("lock");
export const LogoIcon = icon("logo");
export const LogoutIcon = icon("logout");
export const MoreIcon = icon("more");
export const PeopleIcon = icon("users");
export const PlusIcon = icon("plus");
export const RelayIcon = icon("relay");
export const SendIcon = icon("send");
export const SettingsIcon = icon("settings");
export const ShareIcon = icon("share");
export const ShieldIcon = icon("shield");
export const SyncIcon = icon("sync");
export const TrashIcon = icon("trash");
export const UserIcon = icon("user");
