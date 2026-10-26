import { chromium, expect, test, webkit } from '@playwright/test';

import { insecureOriginProxy, remoteOrigin } from './insecure-origin';

for (const [browserName, engine] of [
  ['WebKit', webkit],
  ['Chromium', chromium],
] as const) {
  test(`${browserName} LAN draft mutation order survives held byte preparation and cancel (HS2-2HF3H4)`, async ({
    baseURL,
  }) => {
    const proxy = await insecureOriginProxy(baseURL!),
      browser = await engine.launch();
    try {
      const page = await browser.newPage({ proxy: { server: proxy.url } });
      await page.route(remoteOrigin, (route) =>
        route.fulfill({ contentType: 'text/html', body: '<!doctype html><title>Synthetic draft ordering</title>' }),
      );
      await page.goto(remoteOrigin);
      expect(await page.evaluate(() => isSecureContext)).toBe(false);
      expect(
        await page.evaluate(async () => {
          const modulePath = '/src/workspace-session.ts',
            recoveryPath = '/src/draft-file-recovery.ts',
            storage = (await import(modulePath)) as typeof import('../src/workspace-session'),
            { createDraftFileRecovery } = (await import(recoveryPath)) as typeof import('../src/draft-file-recovery'),
            recovery = createDraftFileRecovery(),
            scope = 'composer:synthetic-order';
          const held = () => {
            let release: () => void = () => undefined,
              started: () => void = () => undefined;
            const gate = new Promise<void>((resolve) => {
                release = resolve;
              }),
              preparing = new Promise<void>((resolve) => {
                started = resolve;
              }),
              file = new File(['older'], 'old.txt'),
              bytes = file.arrayBuffer.bind(file);
            file.arrayBuffer = async () => {
              started();
              await gate;
              return bytes();
            };
            return { file, release, preparing };
          };
          const cancelled = held(),
            stage = recovery.stage(scope, [{ id: 'cancelled', file: cancelled.file }]);
          await cancelled.preparing;
          const remove = recovery.remove(scope, ['cancelled']);
          // An independent key remains writable while the first preparation is held.
          await storage.saveDraftFile(scope, 'barrier', new File(['independent'], 'barrier.txt'));
          cancelled.release();
          await Promise.all([stage, remove]);
          const afterCancel = await storage.loadDraftFiles(scope, [{ id: 'cancelled', name: 'old.txt' }]),
            older = held(),
            first = storage.saveDraftFile(scope, 'replaced', older.file);
          await older.preparing;
          const second = storage.saveDraftFile(scope, 'replaced', new File(['newer'], 'new.txt'));
          await storage.saveDraftFile(scope, 'barrier', new File(['still independent'], 'barrier.txt'));
          older.release();
          await Promise.all([first, second]);
          const replaced = await storage.loadDraftFiles(scope, [{ id: 'replaced', name: 'new.txt' }]),
            failed = new File(['failed'], 'failed.txt');
          failed.arrayBuffer = () => Promise.reject(new Error('Synthetic byte preparation failure'));
          const failedIds = await recovery.stage(scope, [{ id: 'retry', file: failed }]);
          await recovery.stage(scope, [{ id: 'retry', file: new File(['recovered'], 'retry.txt') }]);
          const retried = await storage.loadDraftFiles(scope, [{ id: 'retry', name: 'retry.txt' }]);
          return {
            afterCancel: afterCancel.files.length,
            replaced: await replaced.files[0].file.text(),
            failedIds,
            retried: await retried.files[0].file.text(),
            pendingAfterRetry: recovery.unsavedCount(scope),
          };
        }),
      ).toEqual({
        afterCancel: 0,
        replaced: 'newer',
        failedIds: ['retry'],
        retried: 'recovered',
        pendingAfterRetry: 0,
      });
    } finally {
      await browser.close();
      await proxy.close();
    }
  });

  test(`${browserName} LAN draft bytes survive reload, invalid records, removal and a further edit (HS2-2HF3H4)`, async ({
    baseURL,
  }) => {
    const proxy = await insecureOriginProxy(baseURL!),
      browser = await engine.launch();
    try {
      const page = await browser.newPage({ proxy: { server: proxy.url } });
      // Keep the genuine insecure origin and shipped persistence module, without booting another app.
      await page.route(remoteOrigin, (route) =>
        route.fulfill({ contentType: 'text/html', body: '<!doctype html><title>Synthetic draft records</title>' }),
      );
      await page.goto(remoteOrigin);
      expect(await page.evaluate(() => isSecureContext)).toBe(false);
      const oldReader = await page.evaluate(async (legacy) => {
        const modulePath = '/src/workspace-session.ts',
          storage = (await import(modulePath)) as typeof import('../src/workspace-session');
        await storage.saveDraftFile(
          'composer:synthetic-project',
          'first',
          new File([new Uint8Array([0, 255, 17, 128])], 'synthetic.bin', {
            type: 'application/octet-stream',
            lastModified: 1234,
          }),
        );
        await storage.saveDraftFile(
          'composer:synthetic-project',
          'empty',
          new File([], 'empty.bin', { type: 'application/octet-stream', lastModified: 4321 }),
        );
        const db = await new Promise<IDBDatabase>((resolve, reject) => {
          const request = indexedDB.open('hotsheet-draft-files-v1');
          request.onsuccess = () => {
            resolve(request.result);
          };
          request.onerror = () => {
            reject(request.error ?? new Error('Could not open synthetic draft records.'));
          };
        });
        await new Promise<void>((resolve, reject) => {
          const tx = db.transaction('files', 'readwrite'),
            store = tx.objectStore('files');
          store.put({ blob: 'unknown representation', name: 'invalid.txt' }, 'composer:synthetic-project:invalid');
          store.put(null, 'composer:synthetic-project:null');
          // WebKit cannot create Blob/File records on this origin. Chromium seeds real legacy data;
          // the codec unit regression also covers existing File records and malformed metadata.
          if (legacy)
            store.put(
              { blob: new Blob(['legacy']), name: 'legacy.txt', type: 'text/plain', lastModified: 5678 },
              'composer:synthetic-project:legacy',
            );
          tx.oncomplete = () => {
            resolve();
          };
          tx.onerror = () => {
            reject(tx.error ?? new Error('Could not seed synthetic draft records.'));
          };
        });
        const raw = await new Promise<import('../src/browser-draft-file-contract').BrowserDraftFileRecord>(
          (resolve, reject) => {
            const request = db.transaction('files').objectStore('files').get('composer:synthetic-project:first');
            request.onsuccess = () => {
              resolve(request.result as import('../src/browser-draft-file-contract').BrowserDraftFileRecord);
            };
            request.onerror = () => {
              reject(request.error ?? new Error('Could not read synthetic draft record.'));
            };
          },
        );
        // This is the reader shipped before the byte representation: retain cross-version compatibility.
        const file = new File([raw.blob], raw.name, { type: raw.type, lastModified: raw.lastModified });
        db.close();
        return {
          bytes: Array.from(new Uint8Array(await file.arrayBuffer())),
          name: file.name,
          type: file.type,
          lastModified: file.lastModified,
          size: file.size,
        };
      }, browserName === 'Chromium');
      expect(oldReader).toEqual({
        bytes: [0, 255, 17, 128],
        name: 'synthetic.bin',
        type: 'application/octet-stream',
        lastModified: 1234,
        size: 4,
      });
      await page.reload();
      const restored = await page.evaluate(async (legacy) => {
        const modulePath = '/src/workspace-session.ts',
          storage = (await import(modulePath)) as typeof import('../src/workspace-session'),
          refs = [
            { id: 'first', name: 'synthetic.bin' },
            { id: 'empty', name: 'empty.bin' },
            { id: 'invalid', name: 'invalid.txt' },
            { id: 'null', name: 'null.txt' },
          ];
        if (legacy) refs.push({ id: 'legacy', name: 'legacy.txt' });
        const result = await storage.loadDraftFiles('composer:synthetic-project', refs);
        return {
          failed: result.failed,
          files: await Promise.all(
            result.files.map(async ({ id, file }) => ({
              id,
              bytes: Array.from(new Uint8Array(await file.arrayBuffer())),
              name: file.name,
              type: file.type,
              size: file.size,
              lastModified: file.lastModified,
            })),
          ),
        };
      }, browserName === 'Chromium');
      expect(restored.failed).toEqual([
        { id: 'invalid', name: 'invalid.txt' },
        { id: 'null', name: 'null.txt' },
      ]);
      expect(restored.files[0]).toEqual({
        id: 'first',
        bytes: [0, 255, 17, 128],
        name: 'synthetic.bin',
        type: 'application/octet-stream',
        size: 4,
        lastModified: 1234,
      });
      expect(restored.files[1]).toEqual({
        id: 'empty',
        bytes: [],
        name: 'empty.bin',
        type: 'application/octet-stream',
        size: 0,
        lastModified: 4321,
      });
      if (browserName === 'Chromium')
        expect(restored.files[2]).toEqual({
          id: 'legacy',
          bytes: [108, 101, 103, 97, 99, 121],
          name: 'legacy.txt',
          type: 'text/plain',
          size: 6,
          lastModified: 5678,
        });
      expect(
        await page.evaluate(async () => {
          const modulePath = '/src/workspace-session.ts',
            storage = (await import(modulePath)) as typeof import('../src/workspace-session');
          // A failed read must not remove either malformed record, and another scope stays isolated.
          const unchanged = await storage.loadDraftFiles('composer:synthetic-project', [
            { id: 'invalid', name: 'invalid.txt' },
            { id: 'null', name: 'null.txt' },
          ]);
          await storage.deleteDraftFiles('composer:other-project', ['first']);
          const isolated = await storage.loadDraftFiles('composer:synthetic-project', [
            { id: 'first', name: 'synthetic.bin' },
          ]);
          await storage.deleteDraftFiles('composer:synthetic-project', ['first']);
          const removed = await storage.loadDraftFiles('composer:synthetic-project', [
            { id: 'first', name: 'synthetic.bin' },
          ]);
          await storage.saveDraftFile(
            'composer:synthetic-project',
            'first',
            new File(['next'], 'edited.txt', { type: 'text/plain', lastModified: 9012 }),
          );
          const edited = await storage.loadDraftFiles('composer:synthetic-project', [
            { id: 'first', name: 'edited.txt' },
          ]);
          return {
            stillFailed: unchanged.failed.length,
            isolated: isolated.files.length,
            removed: removed.files.length,
            edited: await edited.files[0].file.text(),
            metadata: [edited.files[0].file.name, edited.files[0].file.type, edited.files[0].file.lastModified],
          };
        }),
      ).toEqual({
        stillFailed: 2,
        isolated: 1,
        removed: 0,
        edited: 'next',
        metadata: ['edited.txt', 'text/plain', 9012],
      });
    } finally {
      await browser.close();
      await proxy.close();
    }
  });
}
