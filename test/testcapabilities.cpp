/*
 *    This software is in the public domain, furnished "as is", without technical
 *    support, and with no warranty, express or implied, as to its usefulness for
 *    any purpose.
 *
 */

#include <QtTest>

#include "account.h"
#include "capabilities.h"

#include "testutils/testutils.h"

using namespace OCC;

class TestCapabilities : public QObject
{
    Q_OBJECT

private Q_SLOTS:
    void testIsClassicServer_data()
    {
        QTest::addColumn<QVariantMap>("caps");
        QTest::addColumn<bool>("valid");
        QTest::addColumn<bool>("classic");

        // oCIS: testCapabilities() has spaces.enabled == true
        QTest::newRow("ocis") << TestUtils::testCapabilities() << true << false;

        // classic oc10: valid capabilities, but no spaces block
        QVariantMap classic = TestUtils::testCapabilities();
        classic.remove(QStringLiteral("spaces"));
        QTest::newRow("classic-no-spaces") << classic << true << true;

        // classic oc10: spaces present but disabled
        QVariantMap disabled = TestUtils::testCapabilities();
        disabled.insert(QStringLiteral("spaces"), QVariantMap{{QStringLiteral("enabled"), QStringLiteral("false")}});
        QTest::newRow("classic-spaces-disabled") << disabled << true << true;

        // empty: not valid, therefore not classic either
        QTest::newRow("empty") << QVariantMap{} << false << false;
    }

    void testIsClassicServer()
    {
        QFETCH(QVariantMap, caps);
        QFETCH(bool, valid);
        QFETCH(bool, classic);

        Capabilities capabilities(QUrl(QStringLiteral("http://localhost/owncloud")), caps);
        QCOMPARE(capabilities.isValid(), valid);
        QCOMPARE(capabilities.isClassicServer(), classic);
    }
};

QTEST_GUILESS_MAIN(TestCapabilities)
#include "testcapabilities.moc"
