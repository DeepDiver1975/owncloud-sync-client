/*
 * Copyright (C) by Hannah von Reth <hannah.vonreth@owncloud.com>
 *
 * This program is free software; you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation; either version 2 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful, but
 * WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY
 * or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License
 * for more details.
 */

#include "spacesmanager.h"

#include "libsync/account.h"
#include "libsync/creds/abstractcredentials.h"
#include "libsync/graphapi/jobs/drives.h"


#include <QJsonDocument>
#include <QJsonObject>
#include <QTimer>

#include <chrono>

using namespace std::chrono_literals;

using namespace OCC;
using namespace GraphApi;

namespace {
constexpr auto refreshTimeoutC = 30s;

// Stable, non-empty sentinel id for the synthetic classic space. It must be non-empty so
// SpacesManager::space() and Folder::space() resolve it. It need not be unique across accounts
// (the manager is per-account).
const auto classicSpaceIdC = QStringLiteral("00000000-0000-0000-0000-000000000000");
}

SpacesManager::SpacesManager(Account *parent)
    : QObject(parent)
    , _account(parent)
    , _refreshTimer(new QTimer(this))
{
    connect(_account, &Account::credentialsFetched, this, &SpacesManager::refresh);

    _refreshTimer->setInterval(refreshTimeoutC);
    // the timer will be restarted once we received drives data
    _refreshTimer->setSingleShot(true);
    connect(_refreshTimer, &QTimer::timeout, this, &SpacesManager::refresh);
}

void SpacesManager::refresh()
{
    if (!_account || !_account->accessManager()) {
        return;
    }
    if (!_account->credentials()->ready()) {
        return;
    }

    if (_account->isClassicServer()) {
        // Classic oc10 has no /graph/v1.0/me/drives endpoint. Synthesize the single WebDAV root and
        // deliberately do not (re)start the refresh timer so we never poll the graph API.
        ensureSyntheticSpace();
        return;
    }

    // TODO: leak the job until we fixed the ownership https://github.com/owncloud/client/issues/11203
    // todo todo: I can't identify a leak here but who knows what lurks in the job handling...validate it's ok, as it seems to be
    auto drivesJob = new Drives(_account, nullptr);
    drivesJob->setTimeout(refreshTimeoutC);
    connect(drivesJob, &Drives::finishedSignal, this, [drivesJob, this] {
        drivesJob->deleteLater();

        // a system which provides multiple personal spaces the name of the drive is always used as display name
        auto hasManyPersonalSpaces = _account->capabilities().spacesSupport().hasMultiplePersonalSpaces;
        QList<Space *> newSpaces;
        QList<QString> deletedSpaces;

        if (drivesJob->httpStatusCode() == 200) {
            QList<QString> oldKeys = _spaces.keys();
            for (const auto &dr : drivesJob->drives()) {
                bool driveDisabled = dr.getRoot().getDeleted().getState() == QLatin1String("trashed");
                // we need to treat any newly disabled spaces as if they were deleted so leave it alone.
                // if an existing space is now disabled it will remain in the old key list for removal, below
                if (driveDisabled)
                    continue;

                auto *space = _spaces.value(dr.getId(), nullptr);
                if (space) {
                    oldKeys.removeOne(dr.getId());
                    bool changed = space->setDrive(dr);
                    if (changed)
                        emit spaceChanged(space);
                } else {
                    space = new Space(this, dr, hasManyPersonalSpaces);
                    _spaces.insert(dr.getId(), space);
                    emit spaceAdded(_account->uuid(), space);
                    newSpaces.append(space);
                }
            }
            for (const QString &id : std::as_const(oldKeys)) {
                auto *oldSpace = _spaces.take(id);
                if (oldSpace) {
                    emit spaceAboutToBeRemoved(_account->uuid(), oldSpace);
                    deletedSpaces.append(id);
                    oldSpace->deleteLater();
                }
            }
            if (!_ready) {
                _ready = true;
                Q_EMIT ready();
            }
        }
        if (!newSpaces.isEmpty())
            emit spacesAdded(_account->uuid(), newSpaces, _spaces.count());
        if (!deletedSpaces.isEmpty())
            emit spacesRemoved(_account->uuid(), deletedSpaces, _spaces.count());
        // todo: remove this once the old accountSettings are gone
        Q_EMIT updated(_account);
        _refreshTimer->start();
    });
    _refreshTimer->stop();
    drivesJob->start();
}

void SpacesManager::ensureSyntheticSpace()
{
    if (!_spaces.isEmpty()) {
        // already built - credentialsFetched may fire again (e.g. token refresh). Just re-announce readiness.
        if (!_ready) {
            _ready = true;
            Q_EMIT ready();
        }
        Q_EMIT updated(_account);
        return;
    }

    auto *space = new Space(this, buildClassicDrive(_account), /*hasManyPersonalSpaces*/ false);
    // key on Space::id() (the root id) so space(id) lookups agree with the persisted folder spaceId.
    _spaces.insert(space->id(), space);
    Q_EMIT spaceAdded(_account->uuid(), space);
    _ready = true;
    Q_EMIT ready();
    Q_EMIT spacesAdded(_account->uuid(), {space}, _spaces.count());
    Q_EMIT updated(_account);
}

OpenAPI::OAIDrive SpacesManager::buildClassicDrive(Account *account)
{
    // The drive must be built from JSON: OAIDrive::setName() does not set the internal "valid" flag,
    // so a setter-built drive fails OAIDrive::isValid() and trips the assert in Space::setDrive().
    const QString davUrl = account->davUrl().toString(QUrl::FullyEncoded);
    const QJsonObject root{
        {QStringLiteral("id"), classicSpaceIdC},
        // Space::setDrive() asserts a non-empty root eTag. It never changes for a classic root.
        {QStringLiteral("eTag"), QStringLiteral("\"classic-root\"")},
        {QStringLiteral("webDavUrl"), davUrl},
    };
    const QJsonObject drive{
        // "name" is the only required property for OAIDrive::isValid().
        {QStringLiteral("name"), tr("ownCloud")},
        {QStringLiteral("id"), classicSpaceIdC},
        // "project" (not "personal") so Space::displayName() returns the name verbatim instead of tr("Personal").
        {QStringLiteral("driveType"), QStringLiteral("project")},
        {QStringLiteral("root"), root},
    };
    return OpenAPI::OAIDrive(QString::fromUtf8(QJsonDocument(drive).toJson(QJsonDocument::Compact)));
}

Space *SpacesManager::space(const QString &id) const
{
    if (id.isEmpty())
        return nullptr;
    return _spaces.value(id, nullptr);
}

Account *SpacesManager::account() const
{
    return _account;
}

QVector<Space *> SpacesManager::spaces() const
{
    return {_spaces.begin(), _spaces.end()};
}
