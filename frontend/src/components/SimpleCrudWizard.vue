<script setup lang="ts" generic="TItem extends { id: number }, TForm extends { name: string }">
import { shallowRef, ref, onMounted } from 'vue'
import ErrorBanner from '@/components/ErrorBanner.vue'
import { describeError } from '@/api/client'

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
const loading = ref(false)
const error = ref<string | null>(null)
const showForm = ref(false)
const editingItem = ref<TItem | null>(null)

const form = ref<TForm>(props.emptyForm())

onMounted(() => {
  void loadItems()
})

async function loadItems() {
  loading.value = true
  error.value = null
  try {
    items.value = await props.load()
  } catch (e) {
    error.value = describeError(e, `Failed to load ${props.entityLabel.toLowerCase()}s`)
  } finally {
    loading.value = false
  }
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
  if (!form.value.name.trim()) {
    error.value = `${props.entityLabel} name is required`
    return
  }

  loading.value = true
  error.value = null

  try {
    if (editingItem.value) {
      const updated = await props.update(editingItem.value.id, form.value)
      items.value = items.value.map((i) => (i.id === updated.id ? updated : i))
    } else {
      const created = await props.create(form.value)
      items.value = [...items.value, created]
    }
    cancelForm()
  } catch (e) {
    error.value = describeError(e, `Failed to save ${props.entityLabel.toLowerCase()}`)
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="flex flex-col gap-6">
    <div class="flex items-center justify-between">
      <h2 class="headline text-xl">{{ entityLabel }} Management</h2>
      <button v-if="!showForm" class="btn-primary" @click="startCreate">+ Create {{ entityLabel }}</button>
    </div>

    <ErrorBanner :message="error" />

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
