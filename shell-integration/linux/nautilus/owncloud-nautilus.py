# Install to: ~/.local/share/nautilus/extensions/owncloud-nautilus.py
# Reload: nautilus -q && nautilus

import gi
gi.require_version('Nautilus', '4.0')
gi.require_version('GLib', '2.0')
gi.require_version('GObject', '2.0')
from gi.repository import Nautilus, GLib, GObject
import dbus

DBUS_SERVICE = 'org.owncloud.FileManager1'
DBUS_PATH = '/org/owncloud/FileManager1'
DBUS_INTERFACE = 'org.owncloud.FileManager1'


class OwnCloudExtension(GObject.GObject, Nautilus.MenuProvider, Nautilus.InfoProvider):
    def __init__(self):
        super().__init__()
        self._iface = None
        self._try_connect()

    def _try_connect(self):
        try:
            bus = dbus.SessionBus()
            obj = bus.get_object(DBUS_SERVICE, DBUS_PATH)
            self._iface = dbus.Interface(obj, DBUS_INTERFACE)
        except dbus.DBusException:
            self._iface = None

    def update_file_info(self, file):
        if self._iface is None:
            self._try_connect()
            return Nautilus.OperationResult.FAILED

        path = file.get_location().get_path()
        if path is None:
            return Nautilus.OperationResult.COMPLETE

        try:
            _status, emblem = self._iface.GetFileStatus(path)
            if emblem:
                file.add_emblem(emblem)
        except dbus.DBusException:
            self._iface = None

        return Nautilus.OperationResult.COMPLETE

    def get_file_items(self, files):
        if not files or self._iface is None:
            return []

        path = files[0].get_location().get_path()
        if path is None:
            return []

        try:
            items_raw = self._iface.GetMenuItems(path)
        except dbus.DBusException:
            return []

        all_paths = [f.get_location().get_path() for f in files if f.get_location().get_path()]
        menu_items = []
        for name, command, enabled in items_raw:
            if not enabled:
                continue
            item = Nautilus.MenuItem(
                name=f'OwnCloud::{command}',
                label=str(name),
                tip='',
                icon='',
            )
            item.connect(
                'activate',
                self._on_menu_item,
                str(command),
                all_paths,
            )
            menu_items.append(item)

        return menu_items

    def _on_menu_item(self, _menu_item, command, paths):
        if self._iface:
            try:
                self._iface.ExecuteCommand(command, paths)
            except dbus.DBusException:
                pass
