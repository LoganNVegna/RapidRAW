import { Plus, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { Rule } from '../../store/useCatalogStore';

const input = 'min-w-0 w-full bg-surface rounded-md px-2 py-2 text-sm outline-none focus:ring-1 focus:ring-accent/50';
const booleans = ['raw', 'edited', 'available'];
const numbers = ['rating', 'iso', 'aperture', 'shutter_speed', 'focal_length'];
const enums: Record<string, string[]> = {
  flag: ['', 'pick', 'reject'],
  review: ['unreviewed', 'reviewed', 'deferred'],
  label: ['', 'red', 'yellow', 'green', 'blue', 'purple'],
};
const newRule = (): Rule => ({ field: 'rating', operator: 'gte', value: '3' });
export function validSmartRules(rules: Rule[], depth = 0): boolean {
  if (depth > 8 || rules.length > 128) return false;
  return rules.every((rule) =>
    rule.rules
      ? rule.rules.length > 0 && validSmartRules(rule.rules, depth + 1)
      : (rule.field !== 'metadata' || !!rule.key?.trim()) &&
        (['exists', 'missing'].includes(rule.operator) || !!rule.value.trim() || !!enums[rule.field]),
  );
}

export default function SmartRuleBuilder({
  rules,
  mode,
  onChange,
  depth = 0,
}: {
  rules: Rule[];
  mode: string;
  depth?: number;
  onChange: (rules: Rule[], mode: string) => void;
}) {
  const { t } = useTranslation();
  const update = (index: number, value: Rule) =>
    onChange(
      rules.map((rule, i) => (i === index ? value : rule)),
      mode,
    );
  const fields = [
    'rating',
    'flag',
    'label',
    'tag',
    'date',
    'camera',
    'lens',
    'raw',
    'extension',
    'metadata',
    'iso',
    'aperture',
    'shutter_speed',
    'focal_length',
    'edited',
    'available',
    'review',
    'path',
  ] as const;
  const labels = [
    'rating',
    'flag',
    'colorLabel',
    'keyword',
    'captureDate',
    'camera',
    'lens',
    'rawFile',
    'fileExtension',
    'metadataField',
    'iso',
    'aperture',
    'exposureSeconds',
    'focalLength',
    'edited',
    'available',
    'reviewStatus',
    'folderPath',
  ] as const;
  return (
    <fieldset className={depth ? 'border-l-2 border-accent/30 pl-3 py-2 space-y-3' : 'space-y-3'}>
      <legend className="sr-only">{t('catalog.conditionGroup')}</legend>
      <div className="flex flex-wrap gap-2 items-center text-sm">
        <span>{t('catalog.match')}</span>
        <select
          aria-label={t('catalog.groupOperator')}
          className={`${input} !w-auto`}
          value={mode}
          onChange={(e) => onChange(rules, e.target.value)}
        >
          <option value="all">{t('catalog.allAnd')}</option>
          <option value="any">{t('catalog.anyOr')}</option>
        </select>
      </div>
      {rules.map((rule, index) =>
        rule.rules ? (
          <div key={index} className="flex items-start gap-2 min-w-0">
            <div className="flex-1 min-w-0">
              <SmartRuleBuilder
                rules={rule.rules}
                mode={rule.mode || 'all'}
                depth={depth + 1}
                onChange={(rules, mode) => update(index, { ...rule, rules, mode })}
              />
            </div>
            <button
              aria-label={t('catalog.removeGroup')}
              className="p-2 text-text-secondary hover:text-text-primary"
              onClick={() =>
                onChange(
                  rules.filter((_, i) => i !== index),
                  mode,
                )
              }
            >
              <X size={16} />
            </button>
          </div>
        ) : (
          <div key={index} className="space-y-2">
            <div className="grid grid-cols-[minmax(100px,1fr)_minmax(95px,1fr)_minmax(80px,1fr)_28px] gap-2 items-center">
              <select
                aria-label={t('catalog.ruleField', { number: index + 1 })}
                className={input}
                value={rule.field}
                onChange={(e) => {
                  const field = e.target.value;
                  update(index, {
                    field,
                    operator: field === 'metadata' ? 'exists' : numbers.includes(field) ? 'gte' : 'eq',
                    value: booleans.includes(field) ? '1' : (enums[field]?.[0] ?? (numbers.includes(field) ? '3' : '')),
                    ...(field === 'metadata' ? { key: 'GPSLatitude' } : {}),
                  });
                }}
              >
                {fields.map((field, i) => (
                  <option key={field} value={field}>
                    {t(`catalog.${labels[i]}`)}
                  </option>
                ))}
              </select>
              <select
                aria-label={t('catalog.ruleCondition', { number: index + 1 })}
                className={input}
                value={rule.operator}
                onChange={(e) => update(index, { ...rule, operator: e.target.value })}
              >
                {(numbers.includes(rule.field)
                  ? ['eq', 'ne', 'gte', 'lte', 'gt', 'lt']
                  : booleans.includes(rule.field) || enums[rule.field]
                    ? ['eq', 'ne']
                    : rule.field === 'date'
                      ? ['eq', 'ne', 'gte', 'lte', 'before', 'after', 'exists', 'missing']
                      : ['eq', 'ne', 'contains', 'not_contains', 'starts', 'ends', 'exists', 'missing']
                ).map((op) => (
                  <option key={op} value={op}>
                    {t(`catalog.operator_${op}` as 'catalog.operator_eq')}
                  </option>
                ))}
              </select>
              {['exists', 'missing'].includes(rule.operator) ? (
                <span className="text-xs text-text-secondary">{t('catalog.nonEmptyMetadata')}</span>
              ) : booleans.includes(rule.field) || enums[rule.field] ? (
                <select
                  aria-label={t('catalog.ruleValue', { number: index + 1 })}
                  className={input}
                  value={rule.value}
                  onChange={(e) => update(index, { ...rule, value: e.target.value })}
                >
                  {(booleans.includes(rule.field) ? ['1', '0'] : enums[rule.field]).map((value) => (
                    <option key={value} value={value}>
                      {booleans.includes(rule.field)
                        ? t(value === '1' ? 'catalog.yes' : 'catalog.no')
                        : value || t('catalog.none')}
                    </option>
                  ))}
                </select>
              ) : (
                <input
                  aria-label={t('catalog.ruleValue', { number: index + 1 })}
                  type={rule.field === 'date' ? 'date' : numbers.includes(rule.field) ? 'number' : 'text'}
                  step="any"
                  className={input}
                  placeholder={rule.field === 'extension' ? t('catalog.fileExtensionExample') : ''}
                  value={rule.value}
                  onChange={(e) => update(index, { ...rule, value: e.target.value })}
                />
              )}
              <button
                aria-label={t('catalog.removeCondition')}
                className="p-1 text-text-secondary hover:text-text-primary"
                onClick={() =>
                  onChange(
                    rules.filter((_, i) => i !== index),
                    mode,
                  )
                }
              >
                <X size={16} />
              </button>
            </div>
            {rule.field === 'metadata' && (
              <label className="flex items-center gap-3 text-xs text-text-secondary">
                {t('catalog.metadataKey')}
                <input
                  aria-label={t('catalog.metadataKey')}
                  list="rapidraw-metadata-fields"
                  className={input}
                  value={rule.key || ''}
                  onChange={(e) => update(index, { ...rule, key: e.target.value })}
                />
              </label>
            )}
          </div>
        ),
      )}
      {!rules.length && <p className="text-xs text-text-secondary">{t('catalog.noConditionsHint')}</p>}
      <div className="flex flex-wrap gap-4 text-sm text-text-secondary">
        <button
          className="flex items-center gap-1 hover:text-text-primary py-1"
          disabled={rules.length >= 128}
          onClick={() => onChange([...rules, newRule()], mode)}
        >
          <Plus size={14} />
          {t('catalog.addCondition')}
        </button>
        {depth < 8 && (
          <button
            className="flex items-center gap-1 hover:text-text-primary py-1"
            onClick={() =>
              onChange([...rules, { field: 'group', operator: '', value: '', mode: 'any', rules: [newRule()] }], mode)
            }
          >
            <Plus size={14} />
            {t('catalog.addGroup')}
          </button>
        )}
      </div>
      {depth === 0 && (
        <datalist id="rapidraw-metadata-fields">
          {[
            'Make',
            'Model',
            'LensModel',
            'DateTimeOriginal',
            'GPSLatitude',
            'GPSLongitude',
            'Artist',
            'Copyright',
            'ImageDescription',
            'UserComment',
            'BodySerialNumber',
            'ImageUniqueID',
          ].map((key) => (
            <option key={key} value={key} />
          ))}
        </datalist>
      )}
    </fieldset>
  );
}
