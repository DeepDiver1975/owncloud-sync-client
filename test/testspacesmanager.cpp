/*
 *    This software is in the public domain, furnished "as is", without technical
 *    support, and with no warranty, express or implied, as to its usefulness for
 *    any purpose.
 *
 */

#include <QtTest>

#include "account.h"
#include "capabilities.h"
#include "graphapi/space.h"
#include "graphapi/spacesmanager.h"

#include "testutils/syncenginetestutils.h"
#include "testutils/testutils.h"

using namespace OCC;

namespace {
// A classic oc10 server reports valid capabilities but no spaces support.
QVariantMap classicCapabilities()
{
    QVariantMap caps = TestUtils::testCapabilities();
    caps.remove(QStringLiteral("spaces"));
    return caps;
}
}

class TestSpacesManager : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    void testClassicSynthesizesSingleSpace()
    {
        auto acc = new OCC::Account(QUuid::createUuid(), QStringLiteral("admin"), QUrl(QStringLiteral("http://localhost/owncloud")));
        std::unique_ptr<OCC::Account> accGuard(acc);

        auto *am = new FakeAM({}, nullptr);
        auto *creds = new FakeCredentials(acc, am, acc);
        acc->setCredentials(creds);
        acc->setCapabilities({acc->url(), classicCapabilities()});

        QVERIFY(acc->isClassicServer());

        auto *mgr = acc->spacesManager();
        QVERIFY(mgr);
        QCOMPARE(mgr->spacesCount(), 0);
        QVERIFY(!mgr->isReady());

        QSignalSpy readySpy(mgr, &GraphApi::SpacesManager::ready);

        // credentialsFetched drives SpacesManager::refresh(). For a classic server it synthesizes a
        // single space locally instead of querying the graph API.
        Q_EMIT creds->fetched();

        QCOMPARE(readySpy.count(), 1);
        QVERIFY(mgr->isReady());
        QCOMPARE(mgr->spacesCount(), 1);

        const auto spaces = mgr->spaces();
        QCOMPARE(spaces.count(), 1);
        auto *space = spaces.first();
        QVERIFY(!space->id().isEmpty());
        // the synthetic space must be resolvable by id (Folder::space() relies on this)
        QCOMPARE(mgr->space(space->id()), space);
        // it wraps the account's classic WebDAV root
        QCOMPARE(space->webDavUrl(), acc->davUrl());
        QVERIFY(!space->disabled());
        QCOMPARE(space->displayName(), QStringLiteral("ownCloud"));
    }

    void testClassicRefreshIsIdempotent()
    {
        auto acc = new OCC::Account(QUuid::createUuid(), QStringLiteral("admin"), QUrl(QStringLiteral("http://localhost/owncloud")));
        std::unique_ptr<OCC::Account> accGuard(acc);

        auto *am = new FakeAM({}, nullptr);
        auto *creds = new FakeCredentials(acc, am, acc);
        acc->setCredentials(creds);
        acc->setCapabilities({acc->url(), classicCapabilities()});

        auto *mgr = acc->spacesManager();
        QSignalSpy addedSpy(mgr, &GraphApi::SpacesManager::spaceAdded);

        Q_EMIT creds->fetched();
        // a second fetch (e.g. token refresh) must not create a duplicate space
        Q_EMIT creds->fetched();

        QCOMPARE(mgr->spacesCount(), 1);
        QCOMPARE(addedSpy.count(), 1);
    }
};

QTEST_GUILESS_MAIN(TestSpacesManager)
#include "testspacesmanager.moc"
