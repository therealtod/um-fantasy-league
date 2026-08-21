<script setup lang="ts" generic="TItem extends { id: number }, TForm extends { name: string }">
import { shallowRef, ref, onMounted } from 'vue'
import ErrorBanner from '@/components/ErrorBanner.vue'
import { useAsyncRequest } from '@/composables/useAsyncRequest'

const props = defineProps<{
  entityLabel: string
  emptyMessage: string
  load: () => Promise<TItem[]>
  create: (form: TForm) => Promise<TItem>
  update: (id: number, form: TForm) => Promise<TItem>
  emptyForm: () => TForm
  toForm: (item: TItem) => TForm
}>()

const items = shallowRef<TItem[]>([])
const { loading, error, violations, run } = useAsyncRequest()
const showForm = ref(false)
const editingItem = ref<TItem | null>(null)

const form = ref<TForm>(props.emptyForm())

onMounted(() => {
  void loadItems()
})

async function loadItems() {
  const result = await run(() => props.load(), `Failed to load ${props.entityLabel.toLowerCase()}s`)
  if (result.ok) items.value = result.value
}

function startCreate() {
  editingItem.value = null
  form.value = props.emptyForm()
  showForm.value = true
}

function startEdit(item: TItem) {
  editingItem.value = item
  form.value = props.toForm(item)
  showForm.value = true
}

function cancelForm() {
  showForm.value = false
  editingItem.value = null
  form.value = props.emptyForm()
}

async function save() {
  violations.value = []

  if (!form.value.name.trim()) {
    error.value = `${props.entityLabel} name is required`
    return
  }

  const editing = editingItem.value
  const fallback = `Failed to save ${props.entityLabel.toLowerCase()}`
  const result = editing
    ? await run(() => props.update(editing.id, form.value), fallback)
    : await run(() => props.create(form.value), fallback)

  if (!result.ok) return
  items.value = editing
    ? items.value.map((i) => (i.id === result.value.id ? result.value : i))
    : [...items.value, result.value]
  cancelForm()
}
</script>

<template>
  <div class="flex flex-col gap-6">
    <div class="flex items-center justify-between">
      <h2 class="headline text-xl">{{ entityLabel }} Management</h2>
      <button v-if="!showForm" class="btn-primary" @click="startCreate">+ Create {{ entityLabel }}</button>
    </div>

    <ErrorBanner :message="error" :violations="violations" />

    <!-- Form -->
    <div v-if="showForm" class="panel flex flex-col gap-5 p-6">
      <h3 class="headline text-lg text-cyan">
        {{ editingItem ? `Edit ${entityLabel}` : `Create New ${entityLabel}` }}
      </h3>

      <slot name="fields" :form="form" />

      <div class="flex justify-end gap-3 pt-2">
        <button class="btn-ghost" :disabled="loading" @click="cancelForm">Cancel</button>
        <button class="btn-primary" :disabled="loading" @click="save">
          {{ loading ? 'Saving...' : `Save ${entityLabel}` }}
        </button>
      </div>
    </div>

    <!-- List -->
    <div v-if="!showForm" class="flex flex-col gap-2">
      <div v-if="items.length === 0" class="p-12 text-center">
        <p class="text-ink-dim">{{ emptyMessage }}</p>
      </div>

      <div
        v-for="item in items"
        :key="item.id"
        class="panel flex flex-wrap items-center justify-between gap-3 p-4"
      >
        <slot name="item" :item="item" />
        <button class="btn-ghost px-4 py-2 text-xs" @click="startEdit(item)">Edit</button>
      </div>
    </div>
  </div>
</template>
