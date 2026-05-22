import { useState } from 'react'
import { motion } from 'framer-motion'
import { Button } from '@/components/ui/button'
import { Tv, Gamepad2, Play, Settings } from 'lucide-react'

const TUTORIAL_STEPS = [
  {
    title: 'Bienvenue sur Twitch Miner V2',
    description: "L'application qui farm automatiquement pour vous en arrière-plan. Voyons comment ça marche.",
    icon: Play,
    color: 'text-purple-400'
  },
  {
    title: 'Drops Miner',
    description: "Le Drop Miner regarde automatiquement les streams qui offrent des drops pour les jeux que vous avez configurés. Il s'arrête dès que vous avez récupéré la récompense.",
    icon: Gamepad2,
    color: 'text-green-400'
  },
  {
    title: 'Points de Chaîne',
    description: "Le Points Miner envoie des requêtes silencieuses pour vous faire gagner des points sur les chaînes de votre choix, même en étant hors-ligne sur Twitch.",
    icon: Tv,
    color: 'text-blue-400'
  },
  {
    title: 'Paramètres',
    description: "Configurez vos jeux prioritaires et vos streamers favoris dans l'onglet Settings. Vous êtes prêt !",
    icon: Settings,
    color: 'text-zinc-400'
  }
];

export function OnboardingTutorial({ onComplete }: { onComplete: () => void }) {
  const [currentStep, setCurrentStep] = useState(0);

  const handleNext = () => {
    if (currentStep < TUTORIAL_STEPS.length - 1) {
      setCurrentStep(prev => prev + 1);
    } else {
      completeTutorial();
    }
  };

  const completeTutorial = () => {
    localStorage.setItem('has_seen_tutorial', 'true');
    onComplete();
  };

  const StepIcon = TUTORIAL_STEPS[currentStep].icon;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-md px-4">
      <motion.div 
        initial={{ scale: 0.9, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        exit={{ scale: 0.9, opacity: 0 }}
        className="w-full max-w-md bg-zinc-950 border border-white/10 rounded-2xl p-6 shadow-2xl relative overflow-hidden"
      >
        <div className="absolute top-0 left-0 w-full h-1 bg-white/5">
          <motion.div 
            className="h-full bg-purple-500" 
            initial={{ width: 0 }}
            animate={{ width: `${((currentStep + 1) / TUTORIAL_STEPS.length) * 100}%` }}
            transition={{ duration: 0.3 }}
          />
        </div>

        <div className="flex flex-col items-center text-center space-y-6 py-6">
          <motion.div 
            key={currentStep}
            initial={{ y: 10, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            className={`w-20 h-20 rounded-full bg-white/5 flex items-center justify-center ${TUTORIAL_STEPS[currentStep].color}`}
          >
            <StepIcon className="w-10 h-10" />
          </motion.div>

          <div className="space-y-2 min-h-[100px]">
            <motion.h2 
              key={`title-${currentStep}`}
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              className="text-xl font-bold text-white"
            >
              {TUTORIAL_STEPS[currentStep].title}
            </motion.h2>
            <motion.p 
              key={`desc-${currentStep}`}
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              className="text-zinc-400 text-sm leading-relaxed"
            >
              {TUTORIAL_STEPS[currentStep].description}
            </motion.p>
          </div>
        </div>

        <div className="flex justify-between items-center mt-4">
          <button 
            onClick={completeTutorial}
            className="text-xs text-zinc-500 hover:text-white transition-colors"
          >
            Passer
          </button>
          <div className="flex gap-2">
            {currentStep > 0 && (
              <Button variant="outline" size="sm" onClick={() => setCurrentStep(p => p - 1)} className="border-white/10 text-zinc-400 bg-transparent hover:bg-white/5">
                Retour
              </Button>
            )}
            <Button size="sm" onClick={handleNext} className="bg-purple-600 hover:bg-purple-700 text-white min-w-[80px]">
              {currentStep === TUTORIAL_STEPS.length - 1 ? 'Commencer' : 'Suivant'}
            </Button>
          </div>
        </div>
      </motion.div>
    </div>
  )
}
