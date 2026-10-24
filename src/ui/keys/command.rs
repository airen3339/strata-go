// SPDX-License-Identifier: MIT

//! Every rebindable command, with its stable id and its default keys in each
//! map. Each catalog constant is its own command for now; handlers still match
//! the constants, and nothing reads the registry yet.

use super::{Binding, bindings::*};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Map {
    Default,
    Tenxer,
}

/// A fixed chord group. Its trigger and steps rebind; the group does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Group {
    Go,
    CopyAs,
    Sort,
    Actions,
    Tabs,
}

impl Group {
    /// The trigger's id and the prefix of each step's id.
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::Go => "go",
            Self::CopyAs => "copy_as",
            Self::Sort => "sort",
            Self::Actions => "actions",
            Self::Tabs => "tabs",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Single,
    Trigger(Group),
    /// Matches only while its group's trigger is pending.
    Step(Group),
}

#[derive(Debug)]
pub(crate) struct Spec {
    pub(crate) command: Command,
    pub(crate) id: &'static str,
    pub(crate) kind: Kind,
    /// Ordered bindings; the first is the one menus show.
    pub(crate) default: &'static [Binding],
    pub(crate) tenxer: &'static [Binding],
}

/// Each line is `Variant "id"`, then `trigger(Group)` or `step(Group)` for a
/// chord key, then the default map's and the 10xer map's constants.
macro_rules! registry {
    (@kind) => { Kind::Single };
    (@kind trigger($group:ident)) => { Kind::Trigger(Group::$group) };
    (@kind step($group:ident)) => { Kind::Step(Group::$group) };
    ($(
        $command:ident $id:literal $($kind:ident($group:ident))?
            [$($default:ident),*] [$($tenxer:ident),*];
    )*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub(crate) enum Command {
            $($command,)*
        }

        pub(crate) const REGISTRY: &[Spec] = &[$(Spec {
            command: Command::$command,
            id: $id,
            kind: registry!(@kind $($kind($group))?),
            default: &[$($default),*],
            tenxer: &[$($tenxer),*],
        },)*];

        /// The catalog constant names behind each spec, by command and map.
        #[cfg(test)]
        const CONSTANTS: &[[&[&str]; 2]] = &[
            $([&[$(stringify!($default)),*], &[$(stringify!($tenxer)),*]],)*
        ];
    };
}

registry! {
    Copy "copy" [COPY] [COPY];
    Cut "cut" [CUT] [CUT];
    Paste "paste" [PASTE] [PASTE];
    SelectAll "select_all" [SELECT_ALL] [SELECT_ALL];
    Undo "undo" [UNDO] [UNDO];
    Redo "redo" [REDO] [REDO];
    RedoY "redo_y" [REDO_Y] [REDO_Y];
    NewFolder "new_folder" [NEW_FOLDER] [NEW_FOLDER];
    NewFolderWithSelection "new_folder_with_selection"
        [NEW_FOLDER_WITH_SELECTION] [NEW_FOLDER_WITH_SELECTION];
    Rename "rename" [RENAME] [RENAME];
    Properties "properties" [PROPERTIES] [PROPERTIES];
    ContextMenu "context_menu" [CONTEXT_MENU] [CONTEXT_MENU];
    ContextMenuF10 "context_menu_f10" [CONTEXT_MENU_F10] [CONTEXT_MENU_F10];
    Search "search" [SEARCH] [SEARCH];
    Location "location" [LOCATION] [LOCATION];
    Refresh "refresh" [REFRESH] [REFRESH];
    ToggleHiddenFiles "toggle_hidden_files" [HIDDEN_FILES] [HIDDEN_FILES];
    HiddenFilesPeriod "hidden_files_period" [HIDDEN_FILES_PERIOD] [HIDDEN_FILES_PERIOD];
    ViewColumns "view_columns" [VIEW_COLUMNS] [VIEW_COLUMNS];
    ViewIcons "view_icons" [VIEW_ICONS] [VIEW_ICONS];
    ViewList "view_list" [VIEW_LIST] [VIEW_LIST];
    TextLarger "text_larger" [TEXT_LARGER] [TEXT_LARGER];
    TextLargerEqual "text_larger_equal" [TEXT_LARGER_EQUAL] [TEXT_LARGER_EQUAL];
    TextSmaller "text_smaller" [TEXT_SMALLER] [TEXT_SMALLER];
    TextReset "text_reset" [TEXT_RESET] [TEXT_RESET];
    ThumbnailsLarger "thumbnails_larger" [THUMBNAILS_LARGER] [THUMBNAILS_LARGER];
    ThumbnailsLargerEqual "thumbnails_larger_equal"
        [THUMBNAILS_LARGER_EQUAL] [THUMBNAILS_LARGER_EQUAL];
    ThumbnailsSmaller "thumbnails_smaller" [THUMBNAILS_SMALLER] [THUMBNAILS_SMALLER];
    ThumbnailsReset "thumbnails_reset" [THUMBNAILS_RESET] [THUMBNAILS_RESET];
    FocusSidebar "focus_sidebar" [SIDEBAR_FOCUS] [SIDEBAR_FOCUS];
    Settings "settings" [SETTINGS] [SETTINGS];
    ShortcutReference "shortcut_reference" [REFERENCE] [REFERENCE];
    TenxerMode "tenxer_mode" [TENXER_MODE] [TENXER_MODE];
    TabNew "tab_new" [TAB_NEW] [TAB_NEW];
    TabClose "tab_close" [TAB_CLOSE] [TAB_CLOSE];
    TabNext "tab_next" [TAB_NEXT] [TAB_NEXT];
    TabPrevious "tab_previous" [TAB_PREVIOUS] [TAB_PREVIOUS];
    TabPreviousPageUp "tab_previous_page_up" [TAB_PREVIOUS_PAGE_UP] [TAB_PREVIOUS_PAGE_UP];
    TabNextPageDown "tab_next_page_down" [TAB_NEXT_PAGE_DOWN] [TAB_NEXT_PAGE_DOWN];
    TabMoveLeft "tab_move_left" [TAB_MOVE_LEFT] [TAB_MOVE_LEFT];
    TabMoveRight "tab_move_right" [TAB_MOVE_RIGHT] [TAB_MOVE_RIGHT];
    Duplicate "duplicate" [DUPLICATE] [];
    CopyPath "copy_path" [COPY_PATH] [];
    Pin "pin" [PIN] [];
    RenameR "rename_r" [RENAME_R] [];
    JumpToFolder "jump_to_folder" [JUMP_TO_FOLDER] [];
    Filter "filter" [FILTER] [];
    FilterSlash "filter_slash" [FILTER_SLASH] [];
    Terminal "terminal" [TERMINAL] [TERMINAL];
    ArrowScope "arrow_scope" [ARROW_SCOPE] [];
    ToggleSidebar "toggle_sidebar" [SIDEBAR] [];

    FirstItem "first_item" [FIRST_ITEM] [FIRST_ITEM];
    LastItem "last_item" [LAST_ITEM] [LAST_ITEM];
    FirstItemUp "first_item_up" [FIRST_ITEM_UP] [FIRST_ITEM_UP];
    LastItemDown "last_item_down" [LAST_ITEM_DOWN] [LAST_ITEM_DOWN];
    PageUp "page_up" [PAGE_UP] [PAGE_UP];
    PageDown "page_down" [PAGE_DOWN] [PAGE_DOWN];
    Preview "preview" [SPACE] [SPACE];
    HistoryBack "history_back" [HISTORY_BACK] [HISTORY_BACK];
    HistoryForward "history_forward" [HISTORY_FORWARD] [HISTORY_FORWARD];
    ParentFolder "parent_folder" [PARENT_FOLDER] [PARENT_FOLDER];
    HomeFolder "home_folder" [HOME_FOLDER] [HOME_FOLDER];
    OpenInNewTab "open_in_new_tab" [OPEN_IN_NEW_TAB] [OPEN_IN_NEW_TAB];
    OpenInNewWindow "open_in_new_window" [OPEN_IN_NEW_WINDOW] [OPEN_IN_NEW_WINDOW];
    Complete "complete" [COMPLETE] [COMPLETE];

    MediaPlayPause "media_play_pause" [MEDIA_PLAY_PAUSE] [MEDIA_PLAY_PAUSE];
    MediaSeekBack "media_seek_back" [MEDIA_SEEK_BACK] [MEDIA_SEEK_BACK];
    MediaSeekForward "media_seek_forward" [MEDIA_SEEK_FORWARD] [MEDIA_SEEK_FORWARD];
    MediaVolumeUp "media_volume_up" [MEDIA_VOLUME_UP] [MEDIA_VOLUME_UP];
    MediaVolumeDown "media_volume_down" [MEDIA_VOLUME_DOWN] [MEDIA_VOLUME_DOWN];
    MediaMute "media_mute" [MEDIA_MUTE] [MEDIA_MUTE];
    MediaPrevious "media_previous" [MEDIA_PREVIOUS] [MEDIA_PREVIOUS];
    MediaNext "media_next" [MEDIA_NEXT] [MEDIA_NEXT];
    TenxerMediaMute "tenxer_media_mute" [] [TENXER_MEDIA_MUTE];
    TenxerMediaPrevious "tenxer_media_previous" [] [TENXER_MEDIA_PREVIOUS];
    TenxerMediaNext "tenxer_media_next" [] [TENXER_MEDIA_NEXT];
    MediaSeekBackPage "media_seek_back_page" [MEDIA_SEEK_BACK_PAGE] [MEDIA_SEEK_BACK_PAGE];
    MediaSeekForwardPage "media_seek_forward_page"
        [MEDIA_SEEK_FORWARD_PAGE] [MEDIA_SEEK_FORWARD_PAGE];
    MediaSeekStart "media_seek_start" [MEDIA_SEEK_START] [MEDIA_SEEK_START];
    MediaVolumeStepUp "media_volume_step_up" [MEDIA_VOLUME_STEP_UP] [MEDIA_VOLUME_STEP_UP];
    MediaVolumeStepUpEqual "media_volume_step_up_equal"
        [MEDIA_VOLUME_STEP_UP_EQUAL] [MEDIA_VOLUME_STEP_UP_EQUAL];
    MediaVolumeStepDown "media_volume_step_down"
        [MEDIA_VOLUME_STEP_DOWN] [MEDIA_VOLUME_STEP_DOWN];
    ScrollPreviewDown "scroll_preview_down" [] [SCROLL_PREVIEW_DOWN];
    ScrollPreviewUp "scroll_preview_up" [] [SCROLL_PREVIEW_UP];
    FindInPreview "find_in_preview" [FIND_IN_PREVIEW] [];
    TenxerFindInPreview "tenxer_find_in_preview" [] [TENXER_FIND_IN_PREVIEW];
    PreviousMatch "previous_match" [PREVIOUS_MATCH] [PREVIOUS_MATCH];
    PdfZoomReset "pdf_zoom_reset" [PDF_ZOOM_RESET] [PDF_ZOOM_RESET];

    OpenContainingFolder "open_containing_folder"
        [OPEN_CONTAINING_FOLDER] [OPEN_CONTAINING_FOLDER];
    ConfirmTrash "confirm_trash" [] [TENXER_DELETE_CONFIRM];
    ChooseFolder "choose_folder" [CHOOSE_FOLDER] [CHOOSE_FOLDER];
    ReferenceCategories "reference_categories" [REFERENCE_CATEGORIES] [REFERENCE_CATEGORIES];
    ReferenceSearch "reference_search" [REFERENCE_SEARCH] [REFERENCE_SEARCH];
    ReferenceList "reference_list" [REFERENCE_LIST] [REFERENCE_LIST];

    CloseWindow "close_window" [] [CLOSE_WINDOW];
    TenxerReference "tenxer_reference" [] [TENXER_REFERENCE];
    TenxerSidebar "tenxer_sidebar" [] [TENXER_SIDEBAR];
    Open "open" [] [OPEN];
    Peek "peek" [] [PEEK];
    TenxerLastItem "tenxer_last_item" [TENXER_LAST_ITEM] [TENXER_LAST_ITEM];
    TenxerHistoryBack "tenxer_history_back" [] [TENXER_HISTORY_BACK];
    TenxerHistoryForward "tenxer_history_forward" [] [TENXER_HISTORY_FORWARD];
    HalfPageUp "half_page_up" [] [HALF_PAGE_UP];
    HalfPageDown "half_page_down" [] [HALF_PAGE_DOWN];
    TenxerPageUp "tenxer_page_up" [] [TENXER_PAGE_UP];
    TenxerPageDown "tenxer_page_down" [] [TENXER_PAGE_DOWN];
    TenxerPageUpPageUp "tenxer_page_up_page_up" [] [TENXER_PAGE_UP_PAGE_UP];
    TenxerPageDownPageDown "tenxer_page_down_page_down" [] [TENXER_PAGE_DOWN_PAGE_DOWN];
    Yank "yank" [] [YANK];
    YankCut "yank_cut" [] [YANK_CUT];
    Unyank "unyank" [] [UNYANK];
    UnyankX "unyank_x" [] [UNYANK_X];
    PasteKeepBoth "paste_keep_both" [] [PASTE_KEEP_BOTH];
    PasteReplace "paste_replace" [] [PASTE_REPLACE];
    TenxerDelete "tenxer_delete" [] [TENXER_DELETE];
    TenxerDeletePermanently "tenxer_delete_permanently" [] [TENXER_DELETE_PERMANENTLY];
    Create "create" [] [CREATE];
    TenxerRename "tenxer_rename" [] [TENXER_RENAME];
    OpenWith "open_with" [] [OPEN_WITH];
    MoveTo "move_to" [] [MOVE_TO];
    CopyTo "copy_to" [] [COPY_TO];
    Restore "restore" [] [RESTORE];
    TenxerHiddenFiles "tenxer_hidden_files" [] [TENXER_HIDDEN_FILES];
    TenxerThumbnailsLarger "tenxer_thumbnails_larger" [] [TENXER_THUMBNAILS_LARGER];
    TenxerThumbnailsLargerEqual "tenxer_thumbnails_larger_equal"
        [] [TENXER_THUMBNAILS_LARGER_EQUAL];
    TenxerThumbnailsSmaller "tenxer_thumbnails_smaller" [] [TENXER_THUMBNAILS_SMALLER];
    VisualSelect "visual_select" [] [VISUAL_SELECT];
    VisualUnset "visual_unset" [] [VISUAL_UNSET];
    InvertSelection "invert_selection" [] [INVERT_SELECTION];
    Find "find" [] [FIND];
    FindBackward "find_backward" [] [FIND_BACKWARD];
    RepeatFind "repeat_find" [] [REPEAT_FIND];
    RepeatFindReversed "repeat_find_reversed" [] [REPEAT_FIND_REVERSED];
    FilterListing "filter_listing" [] [FILTER_LISTING];
    SearchSubfolders "search_subfolders" [] [SEARCH_SUBFOLDERS];
    JumpVisited "jump_visited" [] [JUMP_VISITED];
    JumpRecent "jump_recent" [] [JUMP_RECENT];

    Go "go" trigger(Go) [] [GO];
    GoFirst "go.first" step(Go) [] [GO_FIRST];
    GoFollow "go.follow" step(Go) [] [GO_FOLLOW];
    GoPrompt "go.prompt" step(Go) [] [GO_PROMPT];
    GoHome "go.home" step(Go) [] [GO_HOME];
    GoConfig "go.config" step(Go) [] [GO_CONFIG];
    GoDownloads "go.downloads" step(Go) [] [GO_DOWNLOADS];
    GoDocuments "go.documents" step(Go) [] [GO_DOCUMENTS];
    GoMusic "go.music" step(Go) [] [GO_MUSIC];
    GoPictures "go.pictures" step(Go) [] [GO_PICTURES];
    GoVideos "go.videos" step(Go) [] [GO_VIDEOS];
    GoTrash "go.trash" step(Go) [] [GO_TRASH];
    GoNetwork "go.network" step(Go) [] [GO_NETWORK];
    GoRecent "go.recent" step(Go) [] [GO_RECENT];
    GoPin "go.pin" step(Go) [] [GO_PIN];
    GoUnpin "go.unpin" step(Go) [] [GO_UNPIN];

    Sort "sort" trigger(Sort) [] [SORT_BY];
    SortName "sort.name" step(Sort) [] [SORT_BY_NAME];
    SortModified "sort.modified" step(Sort) [] [SORT_BY_MODIFIED];
    SortSize "sort.size" step(Sort) [] [SORT_BY_SIZE];
    SortType "sort.type" step(Sort) [] [SORT_BY_TYPE];

    CopyAs "copy_as" trigger(CopyAs) [] [COPY_AS];
    CopyAsPath "copy_as.path" step(CopyAs) [] [COPY_AS_PATH];
    CopyAsName "copy_as.name" step(CopyAs) [] [COPY_AS_NAME];

    Actions "actions" trigger(Actions) [] [ACTIONS];
    ActionsTerminal "actions.terminal" step(Actions) [] [ACTIONS_TERMINAL];
    ActionsCompress "actions.compress" step(Actions) [] [ACTIONS_COMPRESS];
    ActionsExtract "actions.extract" step(Actions) [] [ACTIONS_EXTRACT];
    ActionsExtractTo "actions.extract_to" step(Actions) [] [ACTIONS_EXTRACT_TO];

    Tabs "tabs" trigger(Tabs) [] [TAB_CHORD];
    TabsNew "tabs.new" step(Tabs) [] [TAB_CHORD_NEW];
    TabsClose "tabs.close" step(Tabs) [] [TAB_CHORD_CLOSE];
    TabsPrevious "tabs.previous" step(Tabs) [] [TAB_CHORD_PREVIOUS];
}

impl Command {
    fn spec(self) -> &'static Spec {
        &REGISTRY[self as usize]
    }

    /// The stable name settings files store.
    pub(crate) fn id(self) -> &'static str {
        self.spec().id
    }

    pub(crate) fn parse(id: &str) -> Option<Self> {
        REGISTRY
            .iter()
            .find(|spec| spec.id == id)
            .map(|spec| spec.command)
    }

    /// The shipped bindings in `map`, primary first; empty where the map
    /// leaves the command unbound.
    pub(crate) fn defaults(self, map: Map) -> &'static [Binding] {
        let spec = self.spec();
        match map {
            Map::Default => spec.default,
            Map::Tenxer => spec.tenxer,
        }
    }

    pub(crate) fn kind(self) -> Kind {
        self.spec().kind
    }
}

#[cfg(test)]
mod tests;
